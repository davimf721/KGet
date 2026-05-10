//
//  DownloadManager.swift
//  KGet
//
//  Manages downloads by interfacing with the Rust kget binary
//

import Foundation
import Combine
import UserNotifications
import CommonCrypto
import AppKit

// MARK: - Constants

private enum Constants {
    static let progressUpdateInterval: TimeInterval = 0.5
    static let sha256ChunkSize = 1024 * 1024  // 1MB
    static let progressUIUpdateInterval: TimeInterval = 0.1
}

// MARK: - Output Parser

private struct OutputParser {
    
    struct ParsedProgress {
        let percent: Double
        let downloadedSize: String
        let totalSize: String
        let speed: String
        let eta: String
    }
    
    static func parseProgress(from output: String) -> ParsedProgress? {
        guard let percent = extractPercent(from: output) else { return nil }
        return ParsedProgress(
            percent: percent,
            downloadedSize: extractDownloadedSize(from: output),
            totalSize: extractTotalSize(from: output),
            speed: extractSpeed(from: output),
            eta: extractETA(from: output)
        )
    }
    
    static func parseFiles(from output: String) -> [TorrentFile]? {
        guard output.contains("FILES:"),
              let jsonArray = extractJSONArray(from: output, prefix: "FILES: ["),
              let data = jsonArray.data(using: .utf8),
              let files = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else { return nil }
        
        return files.enumerated().map { idx, file in
            TorrentFile(
                id: idx,
                name: file["name"] as? String ?? "file_\(idx)",
                size: (file["size"] as? Int64) ?? Int64(file["size"] as? Int ?? 0),
                downloaded: 0,
                progress: 0
            )
        }
    }
    
    static func parseFileProgress(from output: String) -> [(idx: Int, downloaded: Int64, pct: Double)]? {
        guard output.contains("FILE_PROGRESS:"),
              let jsonArray = extractJSONArray(from: output, prefix: "FILE_PROGRESS: ["),
              let data = jsonArray.data(using: .utf8),
              let files = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else { return nil }
        
        return files.compactMap { file -> (Int, Int64, Double)? in
            guard let idx = file["idx"] as? Int else { return nil }
            let downloaded = (file["downloaded"] as? Int64) ?? Int64(file["downloaded"] as? Int ?? 0)
            let pct = file["pct"] as? Double ?? 0
            return (idx, downloaded, pct)
        }
    }
    
    static func parseLength(from output: String) -> Int64? {
        guard let match = output.range(of: #"Length:\s*(\d+)"#, options: .regularExpression) else { return nil }
        let str = output[match].replacingOccurrences(of: "Length:", with: "").trimmingCharacters(in: .whitespaces)
        return Int64(str)
    }
    
    static func containsError(_ output: String) -> Bool {
        let lowercased = output.lowercased()
        return lowercased.contains("error") || lowercased.contains("failed")
    }
    
    // MARK: - Private Helpers
    
    private static func extractPercent(from output: String) -> Double? {
        guard let match = output.range(of: #"PROGRESS:\s*([\d.]+)%"#, options: .regularExpression) else { return nil }
        let str = output[match]
            .replacingOccurrences(of: "PROGRESS:", with: "")
            .replacingOccurrences(of: "%", with: "")
            .trimmingCharacters(in: .whitespaces)
        return Double(str)
    }
    
    private static func extractDownloadedSize(from output: String) -> String {
        extractSizePair(from: output)?.downloaded ?? ""
    }
    
    private static func extractTotalSize(from output: String) -> String {
        extractSizePair(from: output)?.total ?? ""
    }
    
    private static func extractSizePair(from output: String) -> (downloaded: String, total: String)? {
        guard let match = output.range(of: #"\(([^/]+)/([^)]+)\)"#, options: .regularExpression) else { return nil }
        let str = output[match].replacingOccurrences(of: "(", with: "").replacingOccurrences(of: ")", with: "")
        let parts = str.split(separator: "/")
        guard parts.count == 2 else { return nil }
        return (String(parts[0]).trimmingCharacters(in: .whitespaces),
                String(parts[1]).trimmingCharacters(in: .whitespaces))
    }
    
    private static func extractSpeed(from output: String) -> String {
        guard let match = output.range(of: #"SPEED:\s*([^\s]+/s)"#, options: .regularExpression) else { return "" }
        return output[match].replacingOccurrences(of: "SPEED:", with: "").trimmingCharacters(in: .whitespaces)
    }
    
    private static func extractETA(from output: String) -> String {
        guard let match = output.range(of: #"ETA:\s*([^\n]+)"#, options: .regularExpression) else { return "" }
        let eta = output[match].replacingOccurrences(of: "ETA:", with: "").trimmingCharacters(in: .whitespaces)
        return eta == "--" ? "" : eta
    }
    
    private static func extractJSONArray(from output: String, prefix: String) -> String? {
        guard let start = output.range(of: prefix) else { return nil }
        let afterBracket = output[start.upperBound...]
        guard let end = afterBracket.range(of: "]") else { return nil }
        return "[" + String(afterBracket[..<end.lowerBound]) + "]"
    }
}

// MARK: - Formatters

private struct Formatters {
    static func formatBytes(_ bytes: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
    }
    
    static func formatSpeed(_ bytesPerSecond: Double) -> String {
        switch bytesPerSecond {
        case ..<1024:
            return String(format: "%.0f B/s", bytesPerSecond)
        case ..<(1024 * 1024):
            return String(format: "%.1f KB/s", bytesPerSecond / 1024)
        default:
            return String(format: "%.2f MB/s", bytesPerSecond / 1024 / 1024)
        }
    }
    
    static func formatTime(_ seconds: Double) -> String {
        switch seconds {
        case ..<60:
            return String(format: "%.0fs", seconds)
        case ..<3600:
            return String(format: "%dm %ds", Int(seconds) / 60, Int(seconds) % 60)
        default:
            return String(format: "%dh %dm", Int(seconds) / 3600, (Int(seconds) % 3600) / 60)
        }
    }
}

// MARK: - Filename Extractor

private struct FilenameExtractor {
    static func extract(from urlString: String) -> String {
        urlString.hasPrefix("magnet:") ? extractFromMagnet(urlString) : extractFromURL(urlString)
    }
    
    private static func extractFromMagnet(_ magnet: String) -> String {
        guard let range = magnet.range(of: "dn=") else {
            return "Torrent_\(Int(Date().timeIntervalSince1970))"
        }
        let afterDn = magnet[range.upperBound...]
        let name = afterDn.range(of: "&").map { String(afterDn[..<$0.lowerBound]) } ?? String(afterDn)
        return name.removingPercentEncoding ?? name
    }
    
    private static func extractFromURL(_ urlString: String) -> String {
        guard let url = URL(string: urlString) else { return defaultFilename() }
        let filename = url.lastPathComponent
        return filename.isEmpty || filename == "/" ? defaultFilename() : filename
    }
    
    private static func defaultFilename() -> String {
        "download_\(Int(Date().timeIntervalSince1970))"
    }
}

// MARK: - Input Validation

private struct DownloadValidator {
    static func isSupportedMagnet(_ value: String) -> Bool {
        guard value.lowercased().hasPrefix("magnet:?") else { return false }
        let hexPattern = #"(?i)xt=urn:btih:[a-f0-9]{40}($|&)"#
        let base32Pattern = #"(?i)xt=urn:btih:[a-z2-7]{32}($|&)"#
        let btmhPattern = #"(?i)xt=urn:btmh:[a-z0-9]+($|&)"#
        return value.range(of: hexPattern, options: .regularExpression) != nil
            || value.range(of: base32Pattern, options: .regularExpression) != nil
            || value.range(of: btmhPattern, options: .regularExpression) != nil
    }

    static func validate(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "Enter a URL or magnet link." }
        if trimmed.lowercased().hasPrefix("magnet:") && !isSupportedMagnet(trimmed) {
            return "Invalid magnet link. Expected a btih/btmh info hash."
        }
        guard trimmed.lowercased().hasPrefix("http://")
            || trimmed.lowercased().hasPrefix("https://")
            || trimmed.lowercased().hasPrefix("ftp://")
            || trimmed.lowercased().hasPrefix("sftp://")
            || trimmed.lowercased().hasPrefix("magnet:?")
        else {
            return "Supported: http, https, ftp, sftp, and magnet links."
        }
        return nil
    }

    static func normalize(_ value: String) -> String {
        value.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

// MARK: - SHA256 Calculator

private class SHA256Calculator {
    private let filePath: String
    private let fileSize: Int64
    private let onProgress: (Double) -> Void
    
    init(filePath: String, fileSize: Int64, onProgress: @escaping (Double) -> Void) {
        self.filePath = filePath
        self.fileSize = fileSize
        self.onProgress = onProgress
    }
    
    func calculate() -> String? {
        guard let fileHandle = FileHandle(forReadingAtPath: filePath) else { return nil }
        defer { try? fileHandle.close() }
        
        var context = CC_SHA256_CTX()
        CC_SHA256_Init(&context)
        
        var bytesRead: Int64 = 0
        var lastProgressUpdate = Date()
        
        while true {
            let data = fileHandle.readData(ofLength: Constants.sha256ChunkSize)
            guard !data.isEmpty else { break }
            
            data.withUnsafeBytes { _ = CC_SHA256_Update(&context, $0.baseAddress, CC_LONG(data.count)) }
            bytesRead += Int64(data.count)
            
            let now = Date()
            if now.timeIntervalSince(lastProgressUpdate) >= Constants.progressUIUpdateInterval {
                onProgress(fileSize > 0 ? Double(bytesRead) / Double(fileSize) * 100 : 0)
                lastProgressUpdate = now
            }
            
            if bytesRead >= fileSize { break }
        }
        
        var digest = [UInt8](repeating: 0, count: Int(CC_SHA256_DIGEST_LENGTH))
        CC_SHA256_Final(&digest, &context)
        return digest.map { String(format: "%02x", $0) }.joined()
    }
}

// MARK: - Binary Locator

private struct BinaryLocator {
    static func findKgetBinary() -> String? {
        let paths: [String?] = [
            Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("kget-bin").path,
            "/usr/local/bin/kget",
            FileManager.default.currentDirectoryPath + "/target/release/kget",
            "./target/release/kget"
        ]
        return paths.compactMap { $0 }.first { FileManager.default.fileExists(atPath: $0) }
    }
}

// MARK: - Supporting Types

private struct DownloadConfiguration {
    let url: String
    let outputPath: String
    let advanced: Bool
    let verifyISO: Bool
    let verifyISOIntegrity: Bool
    let expectedSHA256: String?
    
    var isTorrent: Bool { url.hasPrefix("magnet:") }
    var effectiveAdvanced: Bool { advanced && !isTorrent }
    var shouldVerify: Bool { verifyISO || (verifyISOIntegrity && url.lowercased().hasSuffix(".iso")) }
}

private struct ProgressMonitorConfig {
    let downloadId: UUID
    let filePath: String
    let usesOutputProgress: Bool
}

// MARK: - Download Manager

class DownloadManager: ObservableObject {
    static let shared = DownloadManager()
    
    @Published var downloads: [Download] = []
    @Published var showNewDownloadSheet = false
    @Published var isMenuBarOnly = false
    @Published var defaultDownloadPath: URL
    @Published var verifyISOIntegrity = true
    @Published var selectedDownloadId: UUID?
    @Published var lastStartError: String?
    @Published var showNotifications = true
    @Published var useAdvancedByDefault = false
    @Published var speedLimitKBPerSecond = ""
    
    private var processes: [UUID: Process] = [:]
    private var progressTimers: [UUID: Timer] = [:]
    private var cancellables = Set<AnyCancellable>()
    
    init() {
        defaultDownloadPath = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first!
        loadSettings()
    }
    
    // MARK: - Public API
    
    @discardableResult
    func startDownload(url: String, outputPath: String? = nil, advanced: Bool? = nil, verifyISO: Bool = false, expectedSHA256: String? = nil) -> Bool {
        let normalizedURL = DownloadValidator.normalize(url)
        if let validationError = DownloadValidator.validate(normalizedURL) {
            lastStartError = validationError
            return false
        }

        if let existing = downloads.first(where: { DownloadValidator.normalize($0.url) == normalizedURL }) {
            selectedDownloadId = existing.id
            lastStartError = "This download is already in the list."
            DispatchQueue.main.async {
                if let index = self.downloads.firstIndex(where: { $0.id == existing.id }) {
                    let item = self.downloads.remove(at: index)
                    self.downloads.insert(item, at: 0)
                }
            }
            return false
        }

        let expectedSHA256 = expectedSHA256?.trimmingCharacters(in: .whitespacesAndNewlines)
        lastStartError = nil
        let config = DownloadConfiguration(
            url: normalizedURL,
            outputPath: outputPath ?? defaultDownloadPath.path,
            advanced: advanced ?? useAdvancedByDefault,
            verifyISO: verifyISO,
            verifyISOIntegrity: verifyISOIntegrity,
            expectedSHA256: expectedSHA256?.isEmpty == false ? expectedSHA256 : nil
        )
        
        let download = createDownload(from: config)
        
        DispatchQueue.main.async {
            self.downloads.insert(download, at: 0)
            self.selectedDownloadId = download.id
        }
        
        executeDownload(download, advanced: config.effectiveAdvanced)
        return true
    }
    
    func cancelDownload(_ download: Download) {
        stopProgressTimer(for: download.id)
        terminateProcess(for: download.id)
        updateStatus(for: download.id, to: .cancelled)
    }
    
    func retryDownload(_ download: Download) {
        updateDownload(download.id) { dl in
            dl.status = .pending
            dl.progress = 0
            dl.error = nil
            dl.expectedBytes = 0
        }
        
        if let download = downloads.first(where: { $0.id == download.id }) {
            executeDownload(download, advanced: download.isAdvanced)
        }
    }
    
    func deleteDownload(_ download: Download, deleteFile: Bool = false) {
        cancelDownload(download)
        if deleteFile { try? FileManager.default.removeItem(atPath: download.fullFilePath) }
        DispatchQueue.main.async { self.downloads.removeAll { $0.id == download.id } }
    }
    
    func clearCompleted() {
        downloads.removeAll { [.completed, .cancelled, .failed].contains($0.status) }
    }

    func cancelSelectedDownload() {
        guard let selectedDownloadId,
              let download = downloads.first(where: { $0.id == selectedDownloadId }),
              download.status == .downloading else { return }
        cancelDownload(download)
    }

    func deleteSelectedDownload() {
        guard let selectedDownloadId,
              let download = downloads.first(where: { $0.id == selectedDownloadId }) else { return }
        deleteDownload(download, deleteFile: false)
    }

    func copyURL(_ download: Download) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(download.url, forType: .string)
    }

    func copySHA256(_ download: Download) {
        guard let checksum = download.sha256Checksum else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(checksum, forType: .string)
    }

    func openFile(_ download: Download) {
        guard FileManager.default.fileExists(atPath: download.fullFilePath) else {
            openFolder(download)
            return
        }
        NSWorkspace.shared.open(URL(fileURLWithPath: download.fullFilePath))
    }

    func openFolder(_ download: Download) {
        let fileURL = URL(fileURLWithPath: download.fullFilePath)
        if FileManager.default.fileExists(atPath: download.fullFilePath) {
            NSWorkspace.shared.selectFile(fileURL.path, inFileViewerRootedAtPath: fileURL.deletingLastPathComponent().path)
        } else {
            NSWorkspace.shared.open(URL(fileURLWithPath: download.outputPath))
        }
    }
    
    func saveSettings() {
        UserDefaults.standard.set(defaultDownloadPath.path, forKey: "defaultDownloadPath")
        UserDefaults.standard.set(isMenuBarOnly, forKey: "isMenuBarOnly")
        UserDefaults.standard.set(verifyISOIntegrity, forKey: "verifyISOIntegrity")
        UserDefaults.standard.set(showNotifications, forKey: "showNotifications")
        UserDefaults.standard.set(useAdvancedByDefault, forKey: "useAdvancedByDefault")
        UserDefaults.standard.set(speedLimitKBPerSecond, forKey: "speedLimitKBPerSecond")
    }
    
    // MARK: - Private Helpers
    
    private func createDownload(from config: DownloadConfiguration) -> Download {
        let filename = FilenameExtractor.extract(from: config.url)
        let fullPath = (config.outputPath as NSString).appendingPathComponent(filename)
        
        return Download(
            id: UUID(),
            url: config.url,
            outputPath: config.outputPath,
            fullFilePath: fullPath,
            status: .pending,
            progress: 0,
            speed: "",
            eta: "",
            isISO: config.url.lowercased().hasSuffix(".iso"),
            isTorrent: config.isTorrent,
            verifyIntegrity: config.shouldVerify,
            expectedSHA256: config.expectedSHA256,
            isAdvanced: config.effectiveAdvanced,
            activeConnections: config.advanced ? 4 : 1
        )
    }
    
    private func updateDownload(_ id: UUID, modifier: @escaping (inout Download) -> Void) {
        DispatchQueue.main.async {
            guard let index = self.downloads.firstIndex(where: { $0.id == id }) else { return }
            modifier(&self.downloads[index])
        }
    }
    
    private func updateStatus(for id: UUID, to status: DownloadStatus) {
        updateDownload(id) { $0.status = status }
    }
    
    private func stopProgressTimer(for id: UUID) {
        progressTimers[id]?.invalidate()
        progressTimers.removeValue(forKey: id)
    }
    
    private func terminateProcess(for id: UUID) {
        processes[id]?.terminate()
        processes.removeValue(forKey: id)
    }
    
    private func loadSettings() {
        if let path = UserDefaults.standard.string(forKey: "defaultDownloadPath") {
            defaultDownloadPath = URL(fileURLWithPath: path)
        }
        isMenuBarOnly = UserDefaults.standard.bool(forKey: "isMenuBarOnly")
        if UserDefaults.standard.object(forKey: "verifyISOIntegrity") != nil {
            verifyISOIntegrity = UserDefaults.standard.bool(forKey: "verifyISOIntegrity")
        }
        if UserDefaults.standard.object(forKey: "showNotifications") != nil {
            showNotifications = UserDefaults.standard.bool(forKey: "showNotifications")
        }
        useAdvancedByDefault = UserDefaults.standard.bool(forKey: "useAdvancedByDefault")
        speedLimitKBPerSecond = UserDefaults.standard.string(forKey: "speedLimitKBPerSecond") ?? ""
    }
    
    func verifyISOChecksum(_ download: Download) {
        guard download.status == .completed, download.isISO else { return }
        
        updateDownload(download.id) { dl in
            dl.status = .verifying
            dl.progress = 0
            dl.verificationProgress = 0
        }
        
        performVerification(for: download)
    }
    
    private func performVerification(for download: Download) {
        let downloadId = download.id
        let filePath = download.fullFilePath
        let fileSize = getFileSize(filePath)
        
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let calculator = SHA256Calculator(filePath: filePath, fileSize: fileSize) { progress in
                self?.updateDownload(downloadId) { dl in
                    dl.progress = progress
                    dl.verificationProgress = progress
                }
            }
            
            let checksum = calculator.calculate()
            
            DispatchQueue.main.async {
                self?.updateDownload(downloadId) { dl in
                    dl.sha256Checksum = checksum
                    dl.status = .completed
                    dl.progress = 100
                    dl.verificationProgress = 100
                }
                self?.sendNotification(title: "ISO Verification Complete", body: "SHA256: \(String(checksum?.prefix(16) ?? ""))...")
            }
        }
    }
    
    // MARK: - Download Execution
    
    private func executeDownload(_ download: Download, advanced: Bool) {
        guard let binaryPath = BinaryLocator.findKgetBinary() else {
            updateDownload(download.id) { $0.status = .failed; $0.error = "kget binary not found" }
            return
        }
        
        let process = createProcess(binaryPath: binaryPath, download: download, advanced: advanced)
        setupOutputHandlers(for: process, downloadId: download.id)
        setupTerminationHandler(for: process, downloadId: download.id)
        runProcess(process, for: download)
    }
    
    private func createProcess(binaryPath: String, download: Download, advanced: Bool) -> Process {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binaryPath)
        process.arguments = buildArguments(for: download, advanced: advanced)
        process.environment = buildEnvironment()
        return process
    }
    
    private func buildArguments(for download: Download, advanced: Bool) -> [String] {
        var args = [download.url, "-O", download.fullFilePath]
        if advanced { args.append("-a") }
        if let speedLimit = speedLimitBytesPerSecond() {
            args.append(contentsOf: ["-l", "\(speedLimit)"])
        }
        if let expectedSHA256 = download.expectedSHA256 {
            args.append(contentsOf: ["--sha256", expectedSHA256])
        }
        return args
    }

    private func speedLimitBytesPerSecond() -> Int? {
        let trimmed = speedLimitKBPerSecond.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let kb = Int(trimmed), kb > 0 else { return nil }
        return kb * 1024
    }
    
    private func buildEnvironment() -> [String: String] {
        var env = ProcessInfo.processInfo.environment
        env["LANG"] = "en_US.UTF-8"
        env["TERM"] = "xterm-256color"
        return env
    }
    
    private func setupOutputHandlers(for process: Process, downloadId: UUID) {
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe
        
        var expectedBytes: Int64 = 0
        
        stdoutPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            self?.handleStdout(handle: handle, downloadId: downloadId, expectedBytes: &expectedBytes)
        }
        
        stderrPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            self?.handleStderr(handle: handle, downloadId: downloadId)
        }
    }
    
    private func handleStdout(handle: FileHandle, downloadId: UUID, expectedBytes: inout Int64) {
        let data = handle.availableData
        guard !data.isEmpty, let output = String(data: data, encoding: .utf8) else { return }
        
        processProgressOutput(output, downloadId: downloadId)
        processFilesOutput(output, downloadId: downloadId)
        processFileProgressOutput(output, downloadId: downloadId)
        processLengthOutput(output, downloadId: downloadId, expectedBytes: &expectedBytes)
        processErrorOutput(output, downloadId: downloadId)
    }
    
    private func handleStderr(handle: FileHandle, downloadId: UUID) {
        let data = handle.availableData
        guard !data.isEmpty, let output = String(data: data, encoding: .utf8) else { return }
        processErrorOutput(output, downloadId: downloadId)
    }
    
    private func processProgressOutput(_ output: String, downloadId: UUID) {
        guard let parsed = OutputParser.parseProgress(from: output) else { return }
        
        updateDownload(downloadId) { dl in
            let shouldUpdate = dl.isTorrent || parsed.percent >= dl.progress
            if shouldUpdate { dl.progress = parsed.percent }
            
            dl.status = .downloading
            if !parsed.downloadedSize.isEmpty { dl.downloadedSize = parsed.downloadedSize }
            if !parsed.totalSize.isEmpty { dl.totalSize = parsed.totalSize }
            if !parsed.speed.isEmpty { dl.speed = parsed.speed }
            if !parsed.eta.isEmpty { dl.eta = parsed.eta }
        }
    }
    
    private func processFilesOutput(_ output: String, downloadId: UUID) {
        guard let files = OutputParser.parseFiles(from: output) else { return }
        updateDownload(downloadId) { $0.torrentFiles = files }
    }
    
    private func processFileProgressOutput(_ output: String, downloadId: UUID) {
        guard let fileProgress = OutputParser.parseFileProgress(from: output) else { return }
        
        updateDownload(downloadId) { dl in
            for (idx, downloaded, pct) in fileProgress where idx < dl.torrentFiles.count {
                dl.torrentFiles[idx].downloaded = downloaded
                dl.torrentFiles[idx].progress = pct
            }
        }
    }
    
    private func processLengthOutput(_ output: String, downloadId: UUID, expectedBytes: inout Int64) {
        guard expectedBytes == 0, let bytes = OutputParser.parseLength(from: output) else { return }
        expectedBytes = bytes
        
        updateDownload(downloadId) { dl in
            dl.expectedBytes = bytes
            dl.totalSize = Formatters.formatBytes(bytes)
        }
    }
    
    private func processErrorOutput(_ output: String, downloadId: UUID) {
        guard OutputParser.containsError(output) else { return }
        updateDownload(downloadId) { dl in
            if dl.error == nil { dl.error = output.trimmingCharacters(in: .whitespacesAndNewlines) }
        }
    }
    
    private func setupTerminationHandler(for process: Process, downloadId: UUID) {
        process.terminationHandler = { [weak self] proc in
            self?.cleanupProcess(downloadId: downloadId, process: process)
            self?.finalizeDownload(downloadId: downloadId, exitCode: proc.terminationStatus)
        }
    }
    
    private func cleanupProcess(downloadId: UUID, process: Process) {
        DispatchQueue.main.async { [weak self] in
            self?.stopProgressTimer(for: downloadId)
        }
        
        (process.standardOutput as? Pipe)?.fileHandleForReading.readabilityHandler = nil
        (process.standardError as? Pipe)?.fileHandleForReading.readabilityHandler = nil
        processes.removeValue(forKey: downloadId)
    }
    
    private func finalizeDownload(downloadId: UUID, exitCode: Int32) {
        DispatchQueue.main.async { [weak self] in
            guard let self = self,
                  let index = self.downloads.firstIndex(where: { $0.id == downloadId }),
                  self.downloads[index].status != .cancelled else { return }
            
            let download = self.downloads[index]
            let fileExists = FileManager.default.fileExists(atPath: download.fullFilePath)
            let fileSize = self.getFileSize(download.fullFilePath)
            
            if exitCode == 0 && fileExists && fileSize > 0 {
                self.handleSuccessfulCompletion(at: index, fileSize: fileSize)
            } else {
                self.handleFailedCompletion(at: index, fileExists: fileExists, fileSize: fileSize, exitCode: exitCode)
            }
        }
    }
    
    private func handleSuccessfulCompletion(at index: Int, fileSize: Int64) {
        downloads[index].status = .completed
        downloads[index].progress = 100
        downloads[index].downloadedSize = Formatters.formatBytes(fileSize)
        
        let download = downloads[index]
        if download.verifyIntegrity && download.isISO {
            verifyISOChecksum(download)
        } else {
            sendNotification(title: "Download Complete", body: download.filename)
        }
    }
    
    private func handleFailedCompletion(at index: Int, fileExists: Bool, fileSize: Int64, exitCode: Int32) {
        downloads[index].status = .failed
        guard downloads[index].error == nil else { return }
        downloads[index].error = determineErrorMessage(fileExists: fileExists, fileSize: fileSize, exitCode: exitCode)
    }
    
    private func determineErrorMessage(fileExists: Bool, fileSize: Int64, exitCode: Int32) -> String {
        if !fileExists { return "File was not created" }
        if fileSize == 0 { return "Downloaded file is empty" }
        return "Download failed (exit code: \(exitCode))"
    }
    
    private func runProcess(_ process: Process, for download: Download) {
        do {
            try process.run()
            processes[download.id] = process
            updateStatus(for: download.id, to: .downloading)
            startProgressMonitoring(for: download)
        } catch {
            updateDownload(download.id) { $0.status = .failed; $0.error = error.localizedDescription }
        }
    }
    
    // MARK: - Progress Monitoring
    
    private func startProgressMonitoring(for download: Download) {
        let config = ProgressMonitorConfig(
            downloadId: download.id,
            filePath: download.fullFilePath,
            usesOutputProgress: download.isAdvanced || download.isTorrent
        )
        
        var lastSize: Int64 = 0
        var lastTime = Date()
        
        let timer = Timer.scheduledTimer(withTimeInterval: Constants.progressUpdateInterval, repeats: true) { [weak self] timer in
            guard let self = self else { timer.invalidate(); return }
            
            guard let index = self.downloads.firstIndex(where: { $0.id == config.downloadId }),
                  self.downloads[index].status == .downloading else {
                timer.invalidate()
                self.progressTimers.removeValue(forKey: config.downloadId)
                return
            }
            
            guard !config.usesOutputProgress else {
                lastTime = Date()
                return
            }
            
            self.updateFileBasedProgress(at: index, filePath: config.filePath, lastSize: &lastSize, lastTime: &lastTime)
        }
        
        progressTimers[download.id] = timer
        RunLoop.main.add(timer, forMode: .common)
    }
    
    private func updateFileBasedProgress(at index: Int, filePath: String, lastSize: inout Int64, lastTime: inout Date) {
        let currentTime = Date()
        let currentSize = getFileSize(filePath)
        let expectedBytes = downloads[index].expectedBytes
        
        let timeDiff = currentTime.timeIntervalSince(lastTime)
        let speed = timeDiff > 0 ? Double(currentSize - lastSize) / timeDiff : 0
        let progress = calculateProgress(currentSize: currentSize, expectedBytes: expectedBytes)
        let eta = calculateETA(expectedBytes: expectedBytes, currentSize: currentSize, speed: speed)
        
        downloads[index].progress = progress
        downloads[index].downloadedSize = Formatters.formatBytes(currentSize)
        downloads[index].speed = Formatters.formatSpeed(speed)
        downloads[index].eta = eta
        
        if expectedBytes > 0 {
            downloads[index].totalSize = Formatters.formatBytes(expectedBytes)
        }
        
        lastSize = currentSize
        lastTime = currentTime
    }
    
    private func calculateProgress(currentSize: Int64, expectedBytes: Int64) -> Double {
        if expectedBytes > 0 { return min(Double(currentSize) / Double(expectedBytes) * 100, 100) }
        return currentSize > 0 ? min(Double(currentSize) / 1_000_000 * 5, 95) : 0
    }
    
    private func calculateETA(expectedBytes: Int64, currentSize: Int64, speed: Double) -> String {
        guard expectedBytes > 0, speed > 0 else { return "" }
        return Formatters.formatTime(Double(expectedBytes - currentSize) / speed)
    }
    
    private func getFileSize(_ path: String) -> Int64 {
        (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? Int64) ?? 0
    }
    
    // MARK: - Notifications
    
    private func sendNotification(title: String, body: String) {
        guard showNotifications else { return }
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        content.sound = .default
        
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    }
}

// MARK: - Models

struct TorrentFile: Identifiable {
    let id: Int
    var name: String
    var size: Int64
    var downloaded: Int64
    var progress: Double
    
    var sizeFormatted: String { ByteCountFormatter.string(fromByteCount: size, countStyle: .file) }
    var downloadedFormatted: String { ByteCountFormatter.string(fromByteCount: downloaded, countStyle: .file) }
}

struct Download: Identifiable {
    let id: UUID
    let url: String
    var outputPath: String
    var fullFilePath: String
    var status: DownloadStatus
    var progress: Double
    var speed: String
    var eta: String
    var totalSize: String
    var downloadedSize: String
    var expectedBytes: Int64
    var error: String?
    var isISO: Bool
    var isTorrent: Bool
    var verifyIntegrity: Bool
    var expectedSHA256: String?
    var sha256Checksum: String?
    var isAdvanced: Bool
    var activeConnections: Int
    var verificationProgress: Double
    var torrentFiles: [TorrentFile]
    var isExpanded: Bool
    
    init(id: UUID, url: String, outputPath: String, fullFilePath: String, status: DownloadStatus,
         progress: Double, speed: String, eta: String, totalSize: String = "", downloadedSize: String = "",
         expectedBytes: Int64 = 0, error: String? = nil, isISO: Bool = false, isTorrent: Bool = false,
         verifyIntegrity: Bool = false, expectedSHA256: String? = nil, sha256Checksum: String? = nil, isAdvanced: Bool = false,
         activeConnections: Int = 1, verificationProgress: Double = 0, torrentFiles: [TorrentFile] = [],
         isExpanded: Bool = false) {
        self.id = id
        self.url = url
        self.outputPath = outputPath
        self.fullFilePath = fullFilePath
        self.status = status
        self.progress = progress
        self.speed = speed
        self.eta = eta
        self.totalSize = totalSize
        self.downloadedSize = downloadedSize
        self.expectedBytes = expectedBytes
        self.error = error
        self.isISO = isISO
        self.isTorrent = isTorrent
        self.verifyIntegrity = verifyIntegrity
        self.expectedSHA256 = expectedSHA256
        self.sha256Checksum = sha256Checksum
        self.isAdvanced = isAdvanced
        self.activeConnections = activeConnections
        self.verificationProgress = verificationProgress
        self.torrentFiles = torrentFiles
        self.isExpanded = isExpanded
    }
    
    var filename: String { URL(string: url)?.lastPathComponent ?? "download" }
    var fileExists: Bool { FileManager.default.fileExists(atPath: fullFilePath) }
    var fileSize: String? {
        guard let size = try? FileManager.default.attributesOfItem(atPath: fullFilePath)[.size] as? UInt64 else { return nil }
        return ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)
    }
}

enum DownloadStatus: String, CaseIterable {
    case pending, downloading, verifying, completed, failed, cancelled
    
    var displayName: String { rawValue.capitalized }
    
    var systemImage: String {
        switch self {
        case .pending: return "clock"
        case .downloading: return "arrow.down.circle"
        case .verifying: return "checkmark.shield"
        case .completed: return "checkmark.circle.fill"
        case .failed: return "exclamationmark.circle.fill"
        case .cancelled: return "xmark.circle.fill"
        }
    }
    
    var color: String {
        switch self {
        case .pending: return "gray"
        case .downloading: return "blue"
        case .verifying: return "purple"
        case .completed: return "green"
        case .failed: return "red"
        case .cancelled: return "orange"
        }
    }
}
