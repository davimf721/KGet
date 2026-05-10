//
//  ContentView.swift
//  KGet
//
//  Main window view with logo, progress bars, and file management
//

import SwiftUI

private enum DownloadFilter: String, CaseIterable, Identifiable {
    case all = "All"
    case active = "Active"
    case completed = "Completed"
    case failed = "Failed"

    var id: String { rawValue }
}

struct ContentView: View {
    @EnvironmentObject var downloadManager: DownloadManager
    @State private var urlInput = ""
    @State private var useAdvancedMode = false
    @State private var selectedFilter: DownloadFilter = .all
    @State private var showDeleteAlert = false
    @State private var downloadToDelete: Download?
    @State private var deleteFileAlso = false
    @FocusState private var urlFieldFocused: Bool
    
    var body: some View {
        VStack(spacing: 0) {
            // Header with logo and new download input
            HeaderView(
                urlInput: $urlInput,
                useAdvancedMode: $useAdvancedMode,
                urlFieldFocused: $urlFieldFocused,
                validationError: downloadManager.lastStartError,
                startDownload: startDownload,
                pasteAndDownload: pasteAndDownload
            )
            
            Divider()

            if !downloadManager.downloads.isEmpty {
                filterBar
                    .padding(.horizontal)
                    .padding(.vertical, 8)

                Divider()
            }
            
            // Downloads list
            if downloadManager.downloads.isEmpty {
                EmptyStateView()
            } else if filteredDownloads.isEmpty {
                EmptyFilteredStateView(filter: selectedFilter.rawValue)
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(filteredDownloads) { download in
                            DownloadRowView(
                                download: download,
                                isSelected: downloadManager.selectedDownloadId == download.id,
                                onDelete: { dl in
                                    downloadToDelete = dl
                                    showDeleteAlert = true
                                },
                                onVerify: { dl in
                                    downloadManager.verifyISOChecksum(dl)
                                }
                            )
                            .padding(.horizontal)
                            .onTapGesture {
                                downloadManager.selectedDownloadId = download.id
                            }
                        }
                    }
                    .padding(.vertical, 8)
                }
            }
            
            // Footer
            FooterView()
        }
        .frame(minWidth: 650, minHeight: 450)
        .background(shortcutButtons)
        .sheet(isPresented: $downloadManager.showNewDownloadSheet) {
            NewDownloadSheet()
        }
        .onAppear {
            useAdvancedMode = downloadManager.useAdvancedByDefault
        }
        .alert("Delete Download", isPresented: $showDeleteAlert, presenting: downloadToDelete) { download in
            Button("Delete from List Only", role: .cancel) {
                downloadManager.deleteDownload(download, deleteFile: false)
            }
            Button("Delete File Also", role: .destructive) {
                downloadManager.deleteDownload(download, deleteFile: true)
            }
            Button("Cancel", role: .cancel) {}
        } message: { download in
            Text("Do you want to delete '\(download.filename)' from the list, or also delete the downloaded file?")
        }
    }
    
    private func startDownload() {
        guard !urlInput.isEmpty else { return }
        if downloadManager.startDownload(url: urlInput, advanced: useAdvancedMode) {
            urlInput = ""
        }
    }

    private func pasteAndDownload() {
        guard let string = NSPasteboard.general.string(forType: .string) else { return }
        urlInput = string
        startDownload()
    }

    private var filteredDownloads: [Download] {
        switch selectedFilter {
        case .all:
            return downloadManager.downloads
        case .active:
            return downloadManager.downloads.filter { $0.status == .downloading || $0.status == .verifying || $0.status == .pending }
        case .completed:
            return downloadManager.downloads.filter { $0.status == .completed }
        case .failed:
            return downloadManager.downloads.filter { $0.status == .failed || $0.status == .cancelled }
        }
    }

    private var filterBar: some View {
        HStack {
            Picker("Filter", selection: $selectedFilter) {
                ForEach(DownloadFilter.allCases) { filter in
                    Text(filter.rawValue).tag(filter)
                }
            }
            .pickerStyle(.segmented)
            .frame(width: 360)

            Spacer()
        }
    }

    @ViewBuilder
    private var shortcutButtons: some View {
        Group {
            Button("") { pasteAndDownload() }
                .keyboardShortcut("v", modifiers: .command)
            Button("") { urlFieldFocused = true }
                .keyboardShortcut("l", modifiers: .command)
            Button("") { downloadManager.cancelSelectedDownload() }
                .keyboardShortcut(.escape, modifiers: [])
            Button("") { downloadManager.deleteSelectedDownload() }
                .keyboardShortcut(.delete, modifiers: [])
        }
        .opacity(0)
        .frame(width: 0, height: 0)
    }
}

// MARK: - Header with Logo

struct HeaderView: View {
    @Binding var urlInput: String
    @Binding var useAdvancedMode: Bool
    var urlFieldFocused: FocusState<Bool>.Binding
    let validationError: String?
    let startDownload: () -> Void
    let pasteAndDownload: () -> Void
    
    var body: some View {
        VStack(spacing: 12) {
            // Logo and title row
            HStack(spacing: 16) {
                // Logo image
                LogoView()
                    .frame(width: 48, height: 48)
                
                VStack(alignment: .leading, spacing: 2) {
                    Text("KGet")
                        .font(.largeTitle)
                        .fontWeight(.bold)
                    
                    Text("Modern Download Manager")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
                
                Spacer()
                
                Text("v\(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "1.6.3")")
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color.secondary.opacity(0.1))
                    .cornerRadius(4)
            }
            
            // URL input row
            HStack(spacing: 8) {
                Image(systemName: "link")
                    .foregroundColor(.secondary)
                
                TextField("Enter URL or paste link...", text: $urlInput)
                    .textFieldStyle(.plain)
                    .focused(urlFieldFocused)
                    .onSubmit {
                        startDownload()
                    }
                
                Button(action: pasteAndDownload) {
                    Image(systemName: "doc.on.clipboard")
                }
                .buttonStyle(.borderless)
                .help("Paste from clipboard and start")
                
                Divider()
                    .frame(height: 20)
                
                Toggle("Advanced", isOn: $useAdvancedMode)
                    .toggleStyle(.checkbox)
                    .help("Use parallel chunk downloading")
                
                Button(action: startDownload) {
                    Label("Download", systemImage: "arrow.down.circle.fill")
                }
                .buttonStyle(.borderedProminent)
                .disabled(urlInput.isEmpty)
            }
            .padding(10)
            .background(Color(NSColor.textBackgroundColor))
            .cornerRadius(8)

            if let validationError {
                HStack(spacing: 6) {
                    Image(systemName: "exclamationmark.triangle.fill")
                    Text(validationError)
                }
                .font(.caption)
                .foregroundColor(.red)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding()
        .background(Color(NSColor.controlBackgroundColor))
    }
    
}

struct EmptyFilteredStateView: View {
    let filter: String

    var body: some View {
        VStack(spacing: 10) {
            Spacer()
            Image(systemName: "line.3.horizontal.decrease.circle")
                .font(.system(size: 42))
                .foregroundColor(.secondary)
            Text("No \(filter.lowercased()) downloads")
                .font(.headline)
                .foregroundColor(.secondary)
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - Logo View

struct LogoView: View {
    var body: some View {
        // Try to load logo from bundle Resources
        if let resourcePath = Bundle.main.resourcePath,
           let nsImage = NSImage(contentsOfFile: resourcePath + "/logo.png") {
            Image(nsImage: nsImage)
                .resizable()
                .aspectRatio(contentMode: .fit)
        } else if let bundlePath = Bundle.main.executableURL?.deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Resources/logo.png").path,
                  let nsImage = NSImage(contentsOfFile: bundlePath) {
            Image(nsImage: nsImage)
                .resizable()
                .aspectRatio(contentMode: .fit)
        } else if FileManager.default.fileExists(atPath: "logo.png"),
                  let nsImage = NSImage(contentsOfFile: "logo.png") {
            // Development fallback - current directory
            Image(nsImage: nsImage)
                .resizable()
                .aspectRatio(contentMode: .fit)
        } else {
            // Fallback gradient icon
            ZStack {
                RoundedRectangle(cornerRadius: 10)
                    .fill(
                        LinearGradient(
                            colors: [.blue, .purple],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                
                Image(systemName: "arrow.down.circle.fill")
                    .font(.system(size: 28))
                    .foregroundColor(.white)
            }
        }
    }
}

// MARK: - Empty State

struct EmptyStateView: View {
    var body: some View {
        VStack(spacing: 20) {
            Spacer()
            
            LogoView()
                .frame(width: 80, height: 80)
                .opacity(0.5)
            
            Text("No Downloads")
                .font(.title2)
                .foregroundColor(.secondary)
            
            Text("Paste a URL above to start downloading")
                .font(.caption)
                .foregroundColor(.secondary)
            
            HStack(spacing: 16) {
                VStack {
                    Image(systemName: "globe")
                        .font(.title3)
                    Text("HTTP/HTTPS")
                        .font(.caption2)
                }
                VStack {
                    Image(systemName: "server.rack")
                        .font(.title3)
                    Text("FTP/SFTP")
                        .font(.caption2)
                }
                VStack {
                    Image(systemName: "link")
                        .font(.title3)
                    Text("Magnet")
                        .font(.caption2)
                }
            }
            .foregroundColor(.secondary)
            .padding(.top, 8)
            
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - Download Row with Progress Bar

struct DownloadRowView: View {
    @EnvironmentObject var downloadManager: DownloadManager
    let download: Download
    let isSelected: Bool
    let onDelete: (Download) -> Void
    let onVerify: (Download) -> Void
    
    @State private var isHovered = false
    
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            // Top row: Status icon, filename, actions
            HStack {
                statusIcon
                    .frame(width: 24)
                
                VStack(alignment: .leading, spacing: 2) {
                    HStack {
                        Text(download.filename)
                            .font(.headline)
                            .lineLimit(1)
                        
                        if download.isAdvanced {
                            HStack(spacing: 2) {
                                Image(systemName: "bolt.fill")
                                    .font(.system(size: 8))
                                Text("TURBO")
                            }
                            .font(.caption2)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 2)
                            .background(
                                LinearGradient(
                                    colors: [.orange, .red],
                                    startPoint: .leading,
                                    endPoint: .trailing
                                )
                            )
                            .foregroundColor(.white)
                            .cornerRadius(3)
                        }
                        
                        if download.isISO {
                            Text("ISO")
                                .font(.caption2)
                                .padding(.horizontal, 4)
                                .padding(.vertical, 2)
                                .background(Color.purple.opacity(0.2))
                                .foregroundColor(.purple)
                                .cornerRadius(3)
                        }
                        
                        if download.isTorrent {
                            Text("🧲 TORRENT")
                                .font(.caption2)
                                .padding(.horizontal, 4)
                                .padding(.vertical, 2)
                                .background(Color.green.opacity(0.2))
                                .foregroundColor(.green)
                                .cornerRadius(3)
                        }
                    }
                    
                    Text(download.url)
                        .font(.caption)
                        .foregroundColor(.secondary)
                        .lineLimit(1)
                }
                
                Spacer()
                
                actionButtons
            }
            
            // Progress section (only when downloading or verifying)
            if download.status == .downloading || download.status == .verifying {
                VStack(spacing: 6) {
                    // Verification progress bar
                    if download.status == .verifying {
                        VerificationProgressBar(progress: download.verificationProgress)
                            .frame(height: 10)
                    }
                    // Progress bar - different style for advanced mode
                    else if download.isAdvanced {
                        // Multi-segment progress bar for advanced/parallel download
                        AdvancedProgressBar(
                            progress: download.progress,
                            connections: download.activeConnections
                        )
                        .frame(height: 12)
                    } else {
                        // Standard progress bar
                        GeometryReader { geometry in
                            ZStack(alignment: .leading) {
                                // Background
                                RoundedRectangle(cornerRadius: 4)
                                    .fill(Color.secondary.opacity(0.2))
                                
                                // Progress fill
                                RoundedRectangle(cornerRadius: 4)
                                    .fill(progressGradient)
                                    .frame(width: geometry.size.width * CGFloat(download.progress / 100))
                                    .animation(.easeInOut(duration: 0.3), value: download.progress)
                            }
                        }
                        .frame(height: 8)
                    }
                    
                    // Stats row
                    HStack {
                        // Percentage
                        if download.status == .verifying {
                            Text("\(Int(download.verificationProgress))%")
                                .font(.system(.caption, design: .monospaced))
                                .fontWeight(.semibold)
                                .foregroundColor(.purple)
                        } else {
                            Text("\(Int(download.progress))%")
                                .font(.system(.caption, design: .monospaced))
                                .fontWeight(.semibold)
                        }
                        
                        Spacer()
                        
                        // Show verification status
                        if download.status == .verifying {
                            HStack(spacing: 4) {
                                Image(systemName: "checkmark.shield")
                                    .font(.caption2)
                                Text("Calculating SHA256...")
                                    .font(.caption)
                            }
                            .foregroundColor(.purple)
                        } else {
                            // Downloaded / Total
                            if !download.downloadedSize.isEmpty || !download.totalSize.isEmpty {
                                Text("\(download.downloadedSize) / \(download.totalSize)")
                                    .font(.caption)
                                    .foregroundColor(.secondary)
                            }
                            
                            // Speed
                            if !download.speed.isEmpty {
                                HStack(spacing: 4) {
                                    Image(systemName: "speedometer")
                                        .font(.caption2)
                                    Text(download.speed)
                                        .font(.system(.caption, design: .monospaced))
                                }
                                .foregroundColor(.blue)
                            }
                            
                            // Connections indicator for advanced mode
                            if download.isAdvanced && download.activeConnections > 1 {
                                HStack(spacing: 3) {
                                    Image(systemName: "arrow.triangle.branch")
                                        .font(.caption2)
                                    Text("\(download.activeConnections)x")
                                        .font(.system(.caption, design: .monospaced))
                                }
                                .foregroundColor(.orange)
                                .help("Parallel connections active")
                            }
                            
                            // ETA
                            if !download.eta.isEmpty {
                                HStack(spacing: 4) {
                                    Image(systemName: "clock")
                                        .font(.caption2)
                                    Text(download.eta)
                                        .font(.system(.caption, design: .monospaced))
                                }
                                .foregroundColor(.secondary)
                            }
                        }
                    }
                }
            }
            
            // Torrent files expandable section
            if download.isTorrent && !download.torrentFiles.isEmpty {
                VStack(spacing: 4) {
                    // Expand/collapse button
                    Button(action: {
                        if let index = downloadManager.downloads.firstIndex(where: { $0.id == download.id }) {
                            withAnimation(.easeInOut(duration: 0.2)) {
                                downloadManager.downloads[index].isExpanded.toggle()
                            }
                        }
                    }) {
                        HStack {
                            Image(systemName: download.isExpanded ? "chevron.down" : "chevron.right")
                                .font(.caption)
                            Text("\(download.torrentFiles.count) file(s)")
                                .font(.caption)
                            Spacer()
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .foregroundColor(.secondary)
                    
                    // File list when expanded
                    if download.isExpanded {
                        VStack(spacing: 6) {
                            ForEach(download.torrentFiles) { file in
                                HStack(spacing: 8) {
                                    Image(systemName: "doc")
                                        .font(.caption2)
                                        .foregroundColor(.secondary)
                                    
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(file.name)
                                            .font(.caption)
                                            .lineLimit(1)
                                        
                                        // File progress bar
                                        GeometryReader { geometry in
                                            ZStack(alignment: .leading) {
                                                RoundedRectangle(cornerRadius: 2)
                                                    .fill(Color.secondary.opacity(0.2))
                                                RoundedRectangle(cornerRadius: 2)
                                                    .fill(Color.green)
                                                    .frame(width: geometry.size.width * CGFloat(file.progress / 100))
                                            }
                                        }
                                        .frame(height: 4)
                                    }
                                    
                                    Text("\(Int(file.progress))%")
                                        .font(.system(.caption2, design: .monospaced))
                                        .foregroundColor(.secondary)
                                        .frame(width: 35, alignment: .trailing)
                                    
                                    Text(file.sizeFormatted)
                                        .font(.caption2)
                                        .foregroundColor(.secondary)
                                        .frame(width: 60, alignment: .trailing)
                                }
                            }
                        }
                        .padding(.leading, 16)
                        .padding(.top, 4)
                    }
                }
            }
            
            // Completed info
            if download.status == .completed {
                HStack {
                    if let fileSize = download.fileSize {
                        Label(fileSize, systemImage: "doc")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                    
                    if let checksum = download.sha256Checksum {
                        Spacer()
                        HStack(spacing: 4) {
                            Image(systemName: "checkmark.shield.fill")
                                .foregroundColor(.green)
                            Text("SHA256: \(String(checksum.prefix(12)))...")
                                .font(.system(.caption2, design: .monospaced))
                                .foregroundColor(.secondary)
                        }
                        .help("Full SHA256: \(checksum)")
                    }
                    
                    Spacer()
                    
                    Text("Completed")
                        .font(.caption)
                        .foregroundColor(.green)
                }
            }
            
            // Error message
            if let error = download.error {
                HStack {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundColor(.red)
                    Text(error)
                        .font(.caption)
                        .foregroundColor(.red)
                        .lineLimit(2)
                }
            }
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(isSelected ? Color.accentColor.opacity(0.12) : (isHovered ? Color(NSColor.controlBackgroundColor) : Color(NSColor.textBackgroundColor)))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(isSelected ? Color.accentColor.opacity(0.55) : Color.secondary.opacity(0.1), lineWidth: 1)
        )
        .contextMenu {
            Button("Copy URL") { downloadManager.copyURL(download) }
            if download.status == .completed {
                Button("Open File") { downloadManager.openFile(download) }
                Button("Open Folder") { downloadManager.openFolder(download) }
            }
            if download.sha256Checksum != nil {
                Button("Copy SHA256") { downloadManager.copySHA256(download) }
            }
            if download.status == .failed || download.status == .cancelled || download.status == .completed {
                Button("Restart") { downloadManager.retryDownload(download) }
            }
            Divider()
            Button("Remove", role: .destructive) { onDelete(download) }
        }
        .onHover { hovering in
            isHovered = hovering
        }
    }
    
    private var progressGradient: LinearGradient {
        if download.status == .verifying {
            return LinearGradient(
                colors: [.purple, .pink],
                startPoint: .leading,
                endPoint: .trailing
            )
        }
        return LinearGradient(
            colors: [.blue, .cyan],
            startPoint: .leading,
            endPoint: .trailing
        )
    }
    
    @ViewBuilder
    private var statusIcon: some View {
        let status = download.status
        
        Group {
            switch status {
            case .pending:
                Image(systemName: "clock")
                    .foregroundColor(.gray)
            case .downloading:
                Image(systemName: "arrow.down.circle")
                    .foregroundColor(.blue)
            case .verifying:
                Image(systemName: "checkmark.shield")
                    .foregroundColor(.purple)
            case .completed:
                Image(systemName: "checkmark.circle.fill")
                    .foregroundColor(.green)
            case .failed:
                Image(systemName: "exclamationmark.circle.fill")
                    .foregroundColor(.red)
            case .cancelled:
                Image(systemName: "xmark.circle.fill")
                    .foregroundColor(.orange)
            }
        }
        .font(.title3)
    }
    
    @ViewBuilder
    private var actionButtons: some View {
        HStack(spacing: 6) {
            Button(action: { downloadManager.copyURL(download) }) {
                Image(systemName: "link")
                    .foregroundColor(.secondary)
            }
            .buttonStyle(.plain)
            .help("Copy URL")

            // Cancel button (during download)
            if download.status == .downloading {
                Button(action: { downloadManager.cancelDownload(download) }) {
                    Image(systemName: "xmark.circle")
                        .foregroundColor(.red)
                }
                .buttonStyle(.plain)
                .help("Cancel download")
            }
            
            // Retry button (failed or cancelled)
            if download.status == .failed || download.status == .cancelled {
                Button(action: { downloadManager.retryDownload(download) }) {
                    Image(systemName: "arrow.clockwise.circle")
                        .foregroundColor(.blue)
                }
                .buttonStyle(.plain)
                .help("Retry download")
            }
            
            // Verify ISO button (completed ISO files)
            if download.status == .completed && download.isISO && download.sha256Checksum == nil {
                Button(action: { onVerify(download) }) {
                    Image(systemName: "checkmark.shield")
                        .foregroundColor(.purple)
                }
                .buttonStyle(.plain)
                .help("Verify SHA256 checksum")
            }

            if download.sha256Checksum != nil {
                Button(action: { downloadManager.copySHA256(download) }) {
                    Image(systemName: "number")
                        .foregroundColor(.green)
                }
                .buttonStyle(.plain)
                .help("Copy SHA256")
            }
            
            // Reveal in Finder (completed)
            if download.status == .completed {
                Button(action: { downloadManager.openFile(download) }) {
                    Image(systemName: "doc")
                        .foregroundColor(.secondary)
                }
                .buttonStyle(.plain)
                .help("Open file")

                Button(action: { revealInFinder() }) {
                    Image(systemName: "folder")
                        .foregroundColor(.secondary)
                }
                .buttonStyle(.plain)
                .help("Show in Finder")
            }
            
            // Delete button (always available)
            Button(action: { onDelete(download) }) {
                Image(systemName: "trash")
                    .foregroundColor(.red.opacity(0.7))
            }
            .buttonStyle(.plain)
            .help("Delete")
        }
    }
    
    private func revealInFinder() {
        downloadManager.openFolder(download)
    }
}

// MARK: - Footer

struct FooterView: View {
    @EnvironmentObject var downloadManager: DownloadManager
    
    var activeCount: Int {
        downloadManager.downloads.filter { $0.status == .downloading }.count
    }
    
    var completedCount: Int {
        downloadManager.downloads.filter { $0.status == .completed }.count
    }
    
    var body: some View {
        HStack {
            HStack(spacing: 16) {
                Label("\(activeCount) active", systemImage: "arrow.down")
                    .foregroundColor(activeCount > 0 ? .blue : .secondary)
                
                Label("\(completedCount) completed", systemImage: "checkmark")
                    .foregroundColor(completedCount > 0 ? .green : .secondary)
            }
            .font(.caption)
            
            Spacer()
            
            if completedCount > 0 {
                Button("Clear Completed") {
                    downloadManager.clearCompleted()
                }
                .buttonStyle(.link)
                .font(.caption)
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 10)
        .background(Color(NSColor.controlBackgroundColor))
    }
}

// MARK: - Advanced Progress Bar with Multi-Segment Animation

struct AdvancedProgressBar: View {
    let progress: Double
    let connections: Int
    
    @State private var animationPhase: CGFloat = 0
    @State private var pulsePhase: [CGFloat] = [0, 0, 0, 0]
    
    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                // Background with segment indicators
                HStack(spacing: 2) {
                    ForEach(0..<connections, id: \.self) { index in
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.secondary.opacity(0.15))
                    }
                }
                
                // Multi-color progress fill with segments
                HStack(spacing: 2) {
                    ForEach(0..<connections, id: \.self) { index in
                        let segmentWidth = (geometry.size.width - CGFloat(connections - 1) * 2) / CGFloat(connections)
                        let segmentProgress = calculateSegmentProgress(
                            overallProgress: progress,
                            segmentIndex: index,
                            totalSegments: connections
                        )
                        
                        ZStack {
                            // Segment background glow when active
                            if segmentProgress > 0 && segmentProgress < 100 {
                                RoundedRectangle(cornerRadius: 4)
                                    .fill(segmentColors(for: index)[0].opacity(0.3))
                                    .blur(radius: 2)
                            }
                            
                            // Main progress fill
                            RoundedRectangle(cornerRadius: 4)
                                .fill(
                                    LinearGradient(
                                        colors: segmentColors(for: index),
                                        startPoint: .leading,
                                        endPoint: .trailing
                                    )
                                )
                                .frame(width: max(0, segmentWidth * CGFloat(segmentProgress / 100)))
                                .animation(.easeInOut(duration: 0.3), value: segmentProgress)
                            
                            // Active download pulse effect
                            if segmentProgress > 0 && segmentProgress < 100 {
                                RoundedRectangle(cornerRadius: 4)
                                    .fill(Color.white.opacity(0.4))
                                    .frame(width: max(0, segmentWidth * CGFloat(segmentProgress / 100)))
                                    .scaleEffect(x: 1, y: index < pulsePhase.count ? CGFloat(0.5) + pulsePhase[index] * CGFloat(0.5) : CGFloat(1))
                                    .opacity(index < pulsePhase.count ? Double(1.0 - pulsePhase[index]) : 0.5)
                            }
                            
                            // Shimmer effect for active segments
                            if segmentProgress > 5 && segmentProgress < 95 {
                                RoundedRectangle(cornerRadius: 4)
                                    .fill(
                                        LinearGradient(
                                            colors: [.clear, .white.opacity(0.5), .clear],
                                            startPoint: .leading,
                                            endPoint: .trailing
                                        )
                                    )
                                    .frame(width: max(0, segmentWidth * CGFloat(segmentProgress / 100)))
                                    .mask(
                                        Rectangle()
                                            .fill(
                                                LinearGradient(
                                                    colors: [.clear, .white, .clear],
                                                    startPoint: .leading,
                                                    endPoint: .trailing
                                                )
                                            )
                                            .frame(width: 40)
                                            .offset(x: CGFloat(-segmentWidth/2) + CGFloat(animationPhase) * segmentWidth)
                                    )
                            }
                            
                            // Chunk number indicator
                            if segmentProgress > 20 {
                                Text("C\(index + 1)")
                                    .font(.system(size: 7, weight: .bold, design: .monospaced))
                                    .foregroundColor(.white.opacity(0.7))
                            }
                        }
                        .frame(width: segmentWidth, alignment: .leading)
                    }
                }
                
                // Separator bolts between segments
                HStack(spacing: 0) {
                    ForEach(0..<connections, id: \.self) { index in
                        Spacer()
                        if index < connections - 1 {
                            ZStack {
                                Circle()
                                    .fill(Color.black.opacity(0.5))
                                    .frame(width: 12, height: 12)
                                Image(systemName: "bolt.fill")
                                    .font(.system(size: 7))
                                    .foregroundColor(.yellow)
                            }
                        }
                    }
                    Spacer()
                }
            }
        }
        .onAppear {
            // Shimmer animation
            withAnimation(.linear(duration: 1.2).repeatForever(autoreverses: false)) {
                animationPhase = 1
            }
            // Pulse animations for each segment (staggered)
            for i in 0..<min(4, connections) {
                let delay = Double(i) * 0.2
                DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
                    withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) {
                        if i < pulsePhase.count {
                            pulsePhase[i] = 1
                        }
                    }
                }
            }
        }
    }
    
    private func calculateSegmentProgress(overallProgress: Double, segmentIndex: Int, totalSegments: Int) -> Double {
        // Each segment represents a chunk being downloaded in parallel
        // They download somewhat independently but overall progress drives them
        let segmentSize = 100.0 / Double(totalSegments)
        let segmentStart = Double(segmentIndex) * segmentSize
        let segmentEnd = segmentStart + segmentSize
        
        // Each segment has its own "virtual" progress based on overall
        // Earlier segments tend to be slightly ahead, later ones catch up
        let phaseOffset = Double(segmentIndex) * 3.0 // Slight offset per segment
        _ = overallProgress + phaseOffset - Double(segmentIndex * 2)
        
        // Map overall progress to this segment's fill
        if overallProgress >= segmentEnd {
            return 100 // This segment is fully downloaded
        } else if overallProgress <= segmentStart {
            // Segment hasn't started yet in theory, but in parallel mode
            // all segments start together, just at different rates
            let earlyStart = max(0, overallProgress * 1.1 - Double(segmentIndex) * 5)
            return min(earlyStart, 100)
        } else {
            // Segment is actively downloading
            let withinSegment = (overallProgress - segmentStart) / segmentSize * 100
            // Add some variation to make it look more natural
            let wave = sin(overallProgress / 10.0 + Double(segmentIndex)) * 8
            return min(max(withinSegment + wave, 0), 100)
        }
    }
    
    private func segmentColors(for index: Int) -> [Color] {
        let colorSets: [[Color]] = [
            [.orange, .red],
            [.red, .pink],
            [.pink, .purple],
            [.purple, .blue]
        ]
        return colorSets[index % colorSets.count]
    }
}

// MARK: - Verification Progress Bar with Shield Animation

struct VerificationProgressBar: View {
    let progress: Double
    
    @State private var shimmerPhase: CGFloat = 0
    @State private var pulseScale: CGFloat = 1.0
    
    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                // Background
                RoundedRectangle(cornerRadius: 5)
                    .fill(Color.purple.opacity(0.15))
                
                // Progress fill with gradient
                RoundedRectangle(cornerRadius: 5)
                    .fill(
                        LinearGradient(
                            colors: [.purple, .indigo, .blue],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(width: geometry.size.width * CGFloat(progress / 100))
                    .animation(.easeInOut(duration: 0.2), value: progress)
                
                // Shimmer effect
                if progress > 0 && progress < 100 {
                    RoundedRectangle(cornerRadius: 5)
                        .fill(
                            LinearGradient(
                                colors: [.clear, .white.opacity(0.4), .clear],
                                startPoint: .leading,
                                endPoint: .trailing
                            )
                        )
                        .frame(width: geometry.size.width * CGFloat(progress / 100))
                        .mask(
                            Rectangle()
                                .fill(
                                    LinearGradient(
                                        colors: [.clear, .white, .clear],
                                        startPoint: .leading,
                                        endPoint: .trailing
                                    )
                                )
                                .frame(width: 60)
                                .offset(x: -geometry.size.width + shimmerPhase * geometry.size.width * 2)
                        )
                }
                
                // Shield icon overlay
                HStack {
                    Spacer()
                    Image(systemName: "checkmark.shield.fill")
                        .font(.system(size: 14))
                        .foregroundColor(.white.opacity(0.8))
                        .scaleEffect(pulseScale)
                        .padding(.trailing, 8)
                }
            }
        }
        .onAppear {
            // Shimmer animation
            withAnimation(.linear(duration: 2.0).repeatForever(autoreverses: false)) {
                shimmerPhase = 1
            }
            // Pulse animation
            withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) {
                pulseScale = 1.15
            }
        }
    }
}

// MARK: - Preview

#if DEBUG
#Preview {
    ContentView()
        .environmentObject(DownloadManager.shared)
}
#endif
