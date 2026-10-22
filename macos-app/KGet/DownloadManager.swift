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
import SwiftUI

// MARK: - Constants

private enum Constants {
    static let sha256ChunkSize = 1024 * 1024  // 1MB
    static let progressUIUpdateInterval: TimeInterval = 0.1
}

// MARK: - JSONL Events

/// One line of `kget --jsonl` output (protocol version 1).
private struct KEvent {
    let event: String
    let raw: [String: Any]

    init?(line: String) {
        guard let data = line.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let event = obj["event"] as? String
        else { return nil }
        self.event = event
        self.raw = obj
    }

    func string(_ key: String) -> String? { raw[key] as? String }
    func double(_ key: String) -> Double? { (raw[key] as? NSNumber)?.doubleValue }
    func int64(_ key: String) -> Int64? { (raw[key] as? NSNumber)?.int64Value }
    func bool(_ key: String) -> Bool? { (raw[key] as? NSNumber)?.boolValue }
    func array(_ key: String) -> [[String: Any]] { raw[key] as? [[String: Any]] ?? [] }
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
        let lower = trimmed.lowercased()
        guard lower.hasPrefix("http://")
            || lower.hasPrefix("https://")
            || lower.hasPrefix("ftp://")
            || lower.hasPrefix("sftp://")
            || lower.hasPrefix("webdav://")
            || lower.hasPrefix("webdavs://")
            || lower.hasPrefix("magnet:?")
            || isTorrentFile(trimmed)
        else {
            return "Supported: http, https, ftp, sftp, webdav, magnet links and .torrent files."
        }
        return nil
    }

    static func normalize(_ value: String) -> String {
        value.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Local `.torrent` path or `http(s)://…/x.torrent` URL.
    static func isTorrentFile(_ value: String) -> Bool {
        let lower = value.lowercased()
        let path = lower.split(separator: "?").first.map(String.init) ?? lower
        return path.hasSuffix(".torrent")
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
            "/opt/homebrew/bin/kget",          // Homebrew on Apple Silicon
            "/usr/local/bin/kget",             // Homebrew on Intel
            NSHomeDirectory() + "/.cargo/bin/kget",
        ]
        return paths.compactMap { $0 }.first { FileManager.default.isExecutableFile(atPath: $0) }
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
    
    var isTorrent: Bool { url.lowercased().hasPrefix("magnet:") || DownloadValidator.isTorrentFile(url) }
    var effectiveAdvanced: Bool { advanced && !isTorrent }
    var shouldVerify: Bool { verifyISO || (verifyISOIntegrity && url.lowercased().hasSuffix(".iso")) }
}

// MARK: - Download Manager

/// All mutable state (`downloads`, `processes`, buffers) is only touched on the
/// main queue; process callbacks hop to it with `DispatchQueue.main.async`.
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
    @Published var autoExtract = false
    @Published var videoQuality = "best"
    @Published var ytdlpPath = ""
    @Published var historyEntries: [HistoryEntry] = []
    @Published var clipboardDetectedURL: String?
    @Published var diagnostics = ""

    // Main queue only.
    private var processes: [UUID: Process] = [:]
    private var stdoutBuffers: [UUID: Data] = [:]
    private var stderrBuffers: [UUID: Data] = [:]
    private var lastStderrError: [UUID: String] = [:]
    private var cancellables = Set<AnyCancellable>()
    private var clipboardChangeCount = NSPasteboard.general.changeCount
    private var clipboardTimer: Timer?
    
    init() {
        defaultDownloadPath = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first!
        loadSettings()
        startClipboardMonitoring()
    }
    
    // MARK: - Public API
    
    @discardableResult
    func startDownload(url: String, outputPath: String? = nil, advanced: Bool? = nil, verifyISO: Bool = false, expectedSHA256: String? = nil) -> Bool {
        let normalizedURL = DownloadValidator.normalize(url)
        if let validationError = DownloadValidator.validate(normalizedURL) {
            lastStartError = validationError
            return false
        }

        // Only an active download blocks a new one; finished items can be downloaded again.
        if let existing = downloads.first(where: {
            DownloadValidator.normalize($0.url) == normalizedURL
                && [.pending, .downloading, .verifying, .extracting, .paused].contains($0.status)
        }) {
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
        updateStatus(for: download.id, to: .cancelled)
        terminateProcess(for: download.id)
    }

    /// Pause = SIGINT: kget saves its resume state (.part / .kget) and exits.
    func pauseDownload(_ download: Download) {
        DispatchQueue.main.async {
            guard let process = self.processes[download.id], process.isRunning else { return }
            if let index = self.downloads.firstIndex(where: { $0.id == download.id }) {
                self.downloads[index].status = .paused
                self.downloads[index].speed = ""
                self.downloads[index].eta = ""
            }
            process.interrupt()
        }
    }

    /// Resume = run the same download again with `-c`.
    func resumeDownload(_ download: Download) {
        updateDownload(download.id) { dl in
            dl.status = .pending
            dl.error = nil
        }
        DispatchQueue.main.async {
            if let current = self.downloads.first(where: { $0.id == download.id }) {
                self.executeDownload(current, advanced: current.isAdvanced, extraArgs: ["-c"])
            }
        }
    }

    func retryDownload(_ download: Download) {
        updateDownload(download.id) { dl in
            dl.status = .pending
            dl.progress = 0
            dl.error = nil
            dl.expectedBytes = 0
        }
        DispatchQueue.main.async {
            if let current = self.downloads.first(where: { $0.id == download.id }) {
                // Turbo downloads resume automatically from their .kget state;
                // -c lets simple downloads continue their .part file too.
                self.executeDownload(current, advanced: current.isAdvanced, extraArgs: ["-c"])
            }
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
              download.status == .downloading || download.status == .paused else { return }
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
    
    /// Load the history through `kget --history --json` (stored in SQLite
    /// since KGet 1.8), so the app never depends on the storage format.
    func loadHistory() {
        guard let binaryPath = BinaryLocator.findKgetBinary() else {
            loadLegacyHistoryFile()
            return
        }
        let env = buildEnvironment()
        DispatchQueue.global(qos: .userInitiated).async {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: binaryPath)
            process.arguments = ["--history", "--json", "--history-limit", "1000"]
            process.environment = env
            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = FileHandle.nullDevice
            var entries: [HistoryEntry]?
            do {
                try process.run()
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()
                if process.terminationStatus == 0 {
                    entries = try? JSONDecoder().decode([HistoryEntry].self, from: data)
                }
            } catch {
                entries = nil
            }
            DispatchQueue.main.async {
                if let entries = entries {
                    self.historyEntries = entries.sorted { $0.createdAt > $1.createdAt }
                } else {
                    // kget older than 1.8 (no --json): read its history.json
                    self.loadLegacyHistoryFile()
                }
            }
        }
    }

    private func loadLegacyHistoryFile() {
        let historyURL: URL
        if let dir = ProcessInfo.processInfo.environment["KGET_CONFIG_DIR"], !dir.isEmpty {
            historyURL = URL(fileURLWithPath: dir).appendingPathComponent("history.json")
        } else {
            guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else { return }
            historyURL = appSupport.appendingPathComponent("kget/history.json")
        }
        guard let data = try? Data(contentsOf: historyURL),
              let entries = try? JSONDecoder().decode([HistoryEntry].self, from: data) else { return }
        historyEntries = entries.sorted { $0.createdAt > $1.createdAt }
    }

    func dismissClipboardURL() {
        clipboardDetectedURL = nil
    }

    func isDownloadableURL(_ string: String) -> Bool {
        let t = string.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.hasPrefix("http://") || t.hasPrefix("https://")
            || t.hasPrefix("ftp://") || t.hasPrefix("sftp://")
            || t.hasPrefix("webdav://") || t.hasPrefix("webdavs://")
            || t.lowercased().hasPrefix("magnet:?")
    }

    /// Mirrors `kget::ytdlp::is_video_url` (the binary reports the final
    /// protocol in its `started` event).
    func isVideoURL(_ string: String) -> Bool {
        let hosts = [
            "youtube.com/watch", "youtube.com/shorts", "youtu.be/",
            "vimeo.com/", "dailymotion.com/video", "twitch.tv/",
            "twitter.com/i/status", "x.com/i/status",
            "instagram.com/reel", "instagram.com/p/",
            "tiktok.com/", "facebook.com/watch", "facebook.com/videos",
            "bilibili.com/video", "rumble.com/v", "odysee.com/@",
        ]
        let lower = string.lowercased()
        return hosts.contains(where: { lower.contains($0) })
    }

    func isWebDAVURL(_ string: String) -> Bool {
        let lower = string.lowercased()
        return lower.hasPrefix("webdav://") || lower.hasPrefix("webdavs://")
    }

    func saveSettings() {
        UserDefaults.standard.set(defaultDownloadPath.path, forKey: "defaultDownloadPath")
        UserDefaults.standard.set(isMenuBarOnly, forKey: "isMenuBarOnly")
        UserDefaults.standard.set(verifyISOIntegrity, forKey: "verifyISOIntegrity")
        UserDefaults.standard.set(showNotifications, forKey: "showNotifications")
        UserDefaults.standard.set(useAdvancedByDefault, forKey: "useAdvancedByDefault")
        UserDefaults.standard.set(speedLimitKBPerSecond, forKey: "speedLimitKBPerSecond")
        UserDefaults.standard.set(autoExtract, forKey: "autoExtract")
        UserDefaults.standard.set(videoQuality, forKey: "videoQuality")
        UserDefaults.standard.set(ytdlpPath, forKey: "ytdlpPath")
    }

    /// Run `kget --doctor` with the same environment used for downloads.
    func runDiagnostics() {
        guard let binaryPath = BinaryLocator.findKgetBinary() else {
            diagnostics = "kget binary not found (looked in the app bundle, /opt/homebrew/bin, /usr/local/bin and ~/.cargo/bin)."
            return
        }
        let env = buildEnvironment()
        DispatchQueue.global(qos: .userInitiated).async {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: binaryPath)
            process.arguments = ["--doctor"]
            process.environment = env
            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = pipe
            var output = ""
            do {
                try process.run()
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()
                output = String(data: data, encoding: .utf8) ?? ""
            } catch {
                output = "Failed to run kget: \(error.localizedDescription)"
            }
            DispatchQueue.main.async { self.diagnostics = "Binary: \(binaryPath)\n" + output }
        }
    }
    
    // MARK: - Clipboard Monitoring

    private func startClipboardMonitoring() {
        clipboardTimer = Timer.scheduledTimer(withTimeInterval: 1.5, repeats: true) { [weak self] _ in
            self?.checkClipboard()
        }
        if let timer = clipboardTimer {
            RunLoop.main.add(timer, forMode: .common)
        }
    }

    private func checkClipboard() {
        let currentCount = NSPasteboard.general.changeCount
        guard currentCount != clipboardChangeCount else { return }
        clipboardChangeCount = currentCount
        guard let string = NSPasteboard.general.string(forType: .string) else { return }
        let trimmed = string.trimmingCharacters(in: .whitespacesAndNewlines)
        guard isDownloadableURL(trimmed) else { return }
        guard !downloads.contains(where: { $0.url == trimmed }) else { return }
        guard clipboardDetectedURL != trimmed else { return }
        DispatchQueue.main.async { self.clipboardDetectedURL = trimmed }
    }

    // MARK: - Private Helpers

    private func createDownload(from config: DownloadConfiguration) -> Download {
        let filename = DownloadValidator.isTorrentFile(config.url) && !config.url.contains("://")
            ? ((config.url as NSString).lastPathComponent as NSString).deletingPathExtension
            : FilenameExtractor.extract(from: config.url)
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
    
    private func terminateProcess(for id: UUID) {
        DispatchQueue.main.async {
            // SIGINT lets kget keep its partial data for a later retry.
            self.processes[id]?.interrupt()
        }
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
        if UserDefaults.standard.object(forKey: "autoExtract") != nil {
            autoExtract = UserDefaults.standard.bool(forKey: "autoExtract")
        }
        videoQuality = UserDefaults.standard.string(forKey: "videoQuality") ?? "best"
        ytdlpPath = UserDefaults.standard.string(forKey: "ytdlpPath") ?? ""
    }
    
    func verifyISOChecksum(_ download: Download) {
        guard download.status == .completed, download.isISO else { return }
        
        updateDownload(download.id) { dl in
            dl.status = .verifying
            dl.verifyingAlgorithm = "sha256"
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
    
    private func executeDownload(_ download: Download, advanced: Bool, extraArgs: [String] = []) {
        guard let binaryPath = BinaryLocator.findKgetBinary() else {
            updateDownload(download.id) { $0.status = .failed; $0.error = "kget binary not found" }
            return
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: binaryPath)
        process.arguments = buildArguments(for: download, advanced: advanced) + extraArgs
        process.environment = buildEnvironment()
        setupOutputHandlers(for: process, downloadId: download.id)
        setupTerminationHandler(for: process, downloadId: download.id)
        runProcess(process, for: download)
    }

    private func buildArguments(for download: Download, advanced: Bool) -> [String] {
        // Machine-readable events; never parse localized text.
        var args = [download.url, "--jsonl"]

        if isVideoURL(download.url) {
            // Route through yt-dlp; output dir is the parent folder
            let outputDir = (download.fullFilePath as NSString).deletingLastPathComponent
            return args + ["--ytdlp", "--quality", videoQuality, "-O", outputDir]
        }

        if download.isTorrent {
            args.append(contentsOf: ["-O", download.outputPath])
        } else {
            args.append(contentsOf: ["-O", download.fullFilePath])
        }
        if advanced && !isWebDAVURL(download.url) && !download.isTorrent {
            args.append("-a")
        }

        // Always pass the limit (0 = unlimited) so a value saved by older
        // versions in config.json can never stick.
        args.append(contentsOf: ["-l", "\(speedLimitBytesPerSecond() ?? 0)"])
        if let expectedSHA256 = download.expectedSHA256 {
            args.append(contentsOf: ["--sha256", expectedSHA256])
        }
        if autoExtract && isArchiveFilename(download.filename) {
            args.append("--extract")
        }
        return args
    }

    private func isArchiveFilename(_ name: String) -> Bool {
        let n = name.lowercased()
        return n.hasSuffix(".zip") || n.hasSuffix(".tar.gz") || n.hasSuffix(".tgz")
            || n.hasSuffix(".tar.bz2") || n.hasSuffix(".tbz2")
            || n.hasSuffix(".tar.xz") || n.hasSuffix(".txz")
            || n.hasSuffix(".7z")
    }

    private func speedLimitBytesPerSecond() -> Int? {
        let trimmed = speedLimitKBPerSecond.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let kb = Int(trimmed), kb > 0 else { return nil }
        return kb * 1024
    }

    private func buildEnvironment() -> [String: String] {
        var env = ProcessInfo.processInfo.environment

        // GUI apps get a minimal PATH from launchd; add the usual install
        // locations (Homebrew Apple Silicon/Intel, MacPorts, pipx/pip --user).
        let extraPaths = [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/opt/local/bin",
            NSHomeDirectory() + "/.local/bin",
        ]
        let currentPath = env["PATH"] ?? "/usr/bin:/bin:/usr/sbin:/sbin"
        let existing = Set(currentPath.split(separator: ":").map(String.init))
        let missing = extraPaths.filter { !existing.contains($0) }
        env["PATH"] = (missing + [currentPath]).joined(separator: ":")

        // A yt-dlp path chosen in Settings wins over auto-detection.
        let customYtdlp = ytdlpPath.trimmingCharacters(in: .whitespacesAndNewlines)
        if !customYtdlp.isEmpty {
            env["KGET_YTDLP_PATH"] = (customYtdlp as NSString).expandingTildeInPath
        }

        env["LANG"] = "en_US.UTF-8"
        env["TERM"] = "dumb"
        return env
    }

    private func setupOutputHandlers(for process: Process, downloadId: UUID) {
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        // availableData returns arbitrary chunks: buffer and split on newlines.
        stdoutPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let chunk = handle.availableData
            guard !chunk.isEmpty else { return }
            DispatchQueue.main.async {
                self?.consume(chunk, downloadId: downloadId, isStderr: false)
            }
        }
        stderrPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let chunk = handle.availableData
            guard !chunk.isEmpty else { return }
            DispatchQueue.main.async {
                self?.consume(chunk, downloadId: downloadId, isStderr: true)
            }
        }
    }

    /// Main queue only.
    private func consume(_ chunk: Data, downloadId: UUID, isStderr: Bool) {
        var buffer = (isStderr ? stderrBuffers[downloadId] : stdoutBuffers[downloadId]) ?? Data()
        buffer.append(chunk)
        while let newline = buffer.firstIndex(of: 0x0A) {
            let lineData = buffer[buffer.startIndex..<newline]
            buffer.removeSubrange(buffer.startIndex...newline)
            guard let line = String(data: lineData, encoding: .utf8) else { continue }
            if isStderr {
                handleStderrLine(line, downloadId: downloadId)
            } else {
                handleLine(line, downloadId: downloadId)
            }
        }
        if isStderr { stderrBuffers[downloadId] = buffer } else { stdoutBuffers[downloadId] = buffer }
    }

    /// Fallback error text: the last `Error:` line printed by kget.
    private func handleStderrLine(_ line: String, downloadId: UUID) {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.hasPrefix("Error:") {
            lastStderrError[downloadId] = String(trimmed.dropFirst("Error:".count)).trimmingCharacters(in: .whitespaces)
        }
    }

    /// Main queue only.
    private func handleLine(_ line: String, downloadId: UUID) {
        guard let ev = KEvent(line: line),
              let index = downloads.firstIndex(where: { $0.id == downloadId }) else { return }
        var dl = downloads[index]

        switch ev.event {
        case "started":
            if dl.status == .pending { dl.status = .downloading }
        case "meta":
            if let total = ev.int64("total") {
                dl.expectedBytes = total
                dl.totalSize = Formatters.formatBytes(total)
            }
        case "progress":
            if ![.paused, .cancelled, .verifying, .extracting].contains(dl.status) {
                dl.status = .downloading
            }
            if let percent = ev.double("percent"), dl.isTorrent || percent >= dl.progress {
                dl.progress = percent
            }
            if let bytes = ev.int64("bytes"), bytes > 0 {
                dl.downloadedSize = Formatters.formatBytes(bytes)
            }
            if let total = ev.int64("total") {
                dl.expectedBytes = total
                dl.totalSize = Formatters.formatBytes(total)
            }
            if let speed = ev.double("speed_bps"), speed > 0 {
                dl.speed = Formatters.formatSpeed(speed)
                dl.speedRaw = speed
                dl.speedHistory.append(speed)
                if dl.speedHistory.count > 30 { dl.speedHistory.removeFirst() }
            }
            if let eta = ev.double("eta_secs") {
                dl.eta = Formatters.formatTime(eta)
            }
        case "phase":
            // Structured post-transfer phase (kget 1.8+).
            dl.reportsPhases = true
            switch ev.string("phase") {
            case "verifying":
                dl.status = .verifying
                dl.verifyingAlgorithm = ev.string("algorithm")
            case "extracting":
                dl.status = .extracting
            default:
                break
            }
        case "status":
            // Fallback for binaries older than 1.8, which only announce
            // hashing through a human-readable status message.
            if !dl.reportsPhases, let message = ev.string("message"),
               message.contains("SHA256") || message.contains("Calculating") {
                dl.status = .verifying
                dl.verifyingAlgorithm = "sha256"
            }
        case "files":
            dl.torrentFiles = ev.array("files").enumerated().map { idx, file in
                TorrentFile(
                    id: idx,
                    name: file["name"] as? String ?? "file_\(idx)",
                    size: (file["size"] as? NSNumber)?.int64Value ?? 0,
                    downloaded: 0,
                    progress: 0
                )
            }
        case "file_progress":
            for file in ev.array("files") {
                guard let idx = (file["idx"] as? NSNumber)?.intValue, idx < dl.torrentFiles.count else { continue }
                dl.torrentFiles[idx].downloaded = (file["downloaded"] as? NSNumber)?.int64Value ?? 0
                dl.torrentFiles[idx].progress = (file["pct"] as? NSNumber)?.doubleValue ?? 0
            }
        case "verified":
            if ev.bool("ok") == true, let hash = ev.string("hash"), ev.string("algorithm") == "sha256" {
                dl.sha256Checksum = hash
            }
        case "saved":
            if let path = ev.string("path") { dl.fullFilePath = path }
            if let bytes = ev.int64("bytes") { dl.downloadedSize = Formatters.formatBytes(bytes) }
        case "completed":
            dl.progress = 100
        case "error":
            if let message = ev.string("message") { dl.error = message }
        default:
            break
        }
        downloads[index] = dl
    }

    private func setupTerminationHandler(for process: Process, downloadId: UUID) {
        // terminationHandler runs on a background thread: hop to the main queue.
        process.terminationHandler = { [weak self] proc in
            let status = proc.terminationStatus
            DispatchQueue.main.async {
                self?.cleanupProcess(downloadId: downloadId, process: proc)
                self?.finalizeDownload(downloadId: downloadId, exitCode: status)
            }
        }
    }

    /// Main queue only.
    private func cleanupProcess(downloadId: UUID, process: Process) {
        (process.standardOutput as? Pipe)?.fileHandleForReading.readabilityHandler = nil
        (process.standardError as? Pipe)?.fileHandleForReading.readabilityHandler = nil
        // Flush a last line without a trailing newline.
        if let rest = stdoutBuffers[downloadId], !rest.isEmpty,
           let line = String(data: rest, encoding: .utf8) {
            handleLine(line, downloadId: downloadId)
        }
        if processes[downloadId] === process {
            processes.removeValue(forKey: downloadId)
        }
        stdoutBuffers.removeValue(forKey: downloadId)
        stderrBuffers.removeValue(forKey: downloadId)
    }

    /// Main queue only. Success or failure is decided by the exit code.
    private func finalizeDownload(downloadId: UUID, exitCode: Int32) {
        let stderrError = lastStderrError.removeValue(forKey: downloadId)
        guard let index = downloads.firstIndex(where: { $0.id == downloadId }) else { return }
        let status = downloads[index].status
        if status == .cancelled || status == .paused { return }

        if exitCode == 0 {
            handleSuccessfulCompletion(at: index)
        } else if exitCode == 130 {
            downloads[index].status = .cancelled
        } else {
            downloads[index].status = .failed
            if downloads[index].error == nil {
                downloads[index].error = stderrError ?? "Download failed (exit code: \(exitCode))"
            }
        }
    }

    private func handleSuccessfulCompletion(at index: Int) {
        downloads[index].status = .completed
        downloads[index].progress = 100
        downloads[index].error = nil
        downloads[index].speed = ""
        downloads[index].eta = ""
        let fileSize = getFileSize(downloads[index].fullFilePath)
        if fileSize > 0 {
            downloads[index].downloadedSize = Formatters.formatBytes(fileSize)
        }

        let download = downloads[index]
        // The binary already hashed the file when --sha256 was given.
        if download.verifyIntegrity && download.isISO && download.sha256Checksum == nil {
            verifyISOChecksum(download)
        } else {
            sendNotification(title: "Download Complete", body: download.filename)
        }
    }

    private func runProcess(_ process: Process, for download: Download) {
        DispatchQueue.main.async {
            do {
                try process.run()
                self.processes[download.id] = process
                if let index = self.downloads.firstIndex(where: { $0.id == download.id }) {
                    self.downloads[index].status = .downloading
                }
            } catch {
                if let index = self.downloads.firstIndex(where: { $0.id == download.id }) {
                    self.downloads[index].status = .failed
                    self.downloads[index].error = error.localizedDescription
                }
            }
        }
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
    var speedRaw: Double          // current bytes/s (for sparkline)
    var speedHistory: [Double]    // last 30 readings in bytes/s
    var verifyingAlgorithm: String?  // from `phase` events, e.g. "sha256"
    var reportsPhases: Bool          // the binary sends structured `phase` events

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
        self.speedRaw = 0
        self.speedHistory = []
        self.verifyingAlgorithm = nil
        self.reportsPhases = false
    }
    
    /// Real file name (from `Content-Disposition`/yt-dlp once the binary reports it).
    var filename: String {
        let name = (fullFilePath as NSString).lastPathComponent
        return name.isEmpty ? (URL(string: url)?.lastPathComponent ?? "download") : name
    }
    var fileExists: Bool { FileManager.default.fileExists(atPath: fullFilePath) }
    var fileSize: String? {
        guard let size = try? FileManager.default.attributesOfItem(atPath: fullFilePath)[.size] as? UInt64 else { return nil }
        return ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)
    }
}

enum DownloadStatus: String, CaseIterable {
    case pending, downloading, paused, verifying, extracting, completed, failed, cancelled

    var displayName: String { rawValue.capitalized }

    var systemImage: String {
        switch self {
        case .pending: return "clock"
        case .downloading: return "arrow.down.circle"
        case .paused: return "pause.circle"
        case .verifying: return "checkmark.shield"
        case .extracting: return "archivebox"
        case .completed: return "checkmark.circle.fill"
        case .failed: return "exclamationmark.circle.fill"
        case .cancelled: return "xmark.circle.fill"
        }
    }

    var color: String {
        switch self {
        case .pending: return "gray"
        case .downloading: return "blue"
        case .paused: return "yellow"
        case .verifying: return "purple"
        case .extracting: return "teal"
        case .completed: return "green"
        case .failed: return "red"
        case .cancelled: return "orange"
        }
    }
}

// MARK: - History Entry

struct HistoryEntry: Codable, Identifiable {
    let id: String
    let url: String
    let filename: String
    let outputDir: String
    let status: String
    let bytesTotal: Int64?
    let sha256: String?
    let expectedSha256: String?
    let createdAt: UInt64
    let finishedAt: UInt64?
    let error: String?

    enum CodingKeys: String, CodingKey {
        case id, url, filename, status, error, sha256
        case outputDir = "output_dir"
        case bytesTotal = "bytes_total"
        case expectedSha256 = "expected_sha256"
        case createdAt = "created_at"
        case finishedAt = "finished_at"
    }

    var createdAtDate: Date {
        Date(timeIntervalSince1970: TimeInterval(createdAt))
    }

    var bytesFormatted: String? {
        guard let bytes = bytesTotal else { return nil }
        return ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
    }

    var statusColor: Color {
        switch status {
        case "completed": return .green
        case "failed":    return .red
        case "cancelled": return .orange
        case "handed_off": return .blue
        default:          return .secondary
        }
    }

    var statusIcon: String {
        switch status {
        case "completed": return "checkmark.circle.fill"
        case "failed":    return "xmark.circle.fill"
        case "cancelled": return "minus.circle.fill"
        case "handed_off": return "arrow.up.forward.app"
        default:          return "clock"
        }
    }
}
