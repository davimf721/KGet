//
//  ContentView.swift
//  KGet
//
//  Liquid Glass design — glass pill URL bar, translucent materials, fluid shapes
//

import SwiftUI
import UniformTypeIdentifiers

// MARK: - Sidebar Navigation

private enum SidebarItem: String, CaseIterable, Identifiable, Hashable {
    case all = "All Downloads"
    case active = "Active"
    case completed = "Completed"
    case failed = "Failed"
    case history = "History"

    var id: String { rawValue }

    var systemImage: String {
        switch self {
        case .all:       return "arrow.down.circle"
        case .active:    return "arrow.down"
        case .completed: return "checkmark.circle"
        case .failed:    return "xmark.circle"
        case .history:   return "clock.arrow.circlepath"
        }
    }
}

// MARK: - Liquid Glass Modifiers

struct GlassCard: ViewModifier {
    var cornerRadius: CGFloat = 12
    var isSelected: Bool = false
    var isHovered: Bool = false

    func body(content: Content) -> some View {
        content
            .background {
                ZStack {
                    RoundedRectangle(cornerRadius: cornerRadius)
                        .fill(.ultraThinMaterial)
                    RoundedRectangle(cornerRadius: cornerRadius)
                        .fill(
                            LinearGradient(
                                colors: [
                                    Color.white.opacity(isHovered ? 0.18 : 0.10),
                                    Color.white.opacity(0.02)
                                ],
                                startPoint: .topLeading,
                                endPoint: .bottomTrailing
                            )
                        )
                    if isSelected {
                        RoundedRectangle(cornerRadius: cornerRadius)
                            .fill(Color.accentColor.opacity(0.10))
                    }
                }
            }
            .overlay {
                RoundedRectangle(cornerRadius: cornerRadius)
                    .strokeBorder(
                        LinearGradient(
                            colors: [
                                Color.white.opacity(isSelected ? 0.55 : 0.35),
                                Color.white.opacity(0.08)
                            ],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        ),
                        lineWidth: 0.75
                    )
            }
            .shadow(color: .black.opacity(isHovered ? 0.12 : 0.06), radius: isHovered ? 8 : 4, y: 2)
    }
}

extension View {
    func glassCard(cornerRadius: CGFloat = 12, isSelected: Bool = false, isHovered: Bool = false) -> some View {
        modifier(GlassCard(cornerRadius: cornerRadius, isSelected: isSelected, isHovered: isHovered))
    }
}

// MARK: - Content View

struct ContentView: View {
    @EnvironmentObject var downloadManager: DownloadManager
    @State private var urlInput = ""
    @State private var useAdvancedMode = false
    @State private var selectedSidebar: SidebarItem? = .all
    @State private var showDeleteAlert = false
    @State private var downloadToDelete: Download?
    @State private var isDragTargeted = false
    @FocusState private var urlFieldFocused: Bool

    private var currentFilter: SidebarItem { selectedSidebar ?? .all }

    private var filteredDownloads: [Download] {
        switch currentFilter {
        case .all:       return downloadManager.downloads
        case .active:    return downloadManager.downloads.filter { $0.status == .downloading || $0.status == .verifying || $0.status == .pending }
        case .completed: return downloadManager.downloads.filter { $0.status == .completed }
        case .failed:    return downloadManager.downloads.filter { $0.status == .failed || $0.status == .cancelled }
        case .history:   return []
        }
    }

    var body: some View {
        NavigationSplitView {
            sidebarView
        } detail: {
            detailView
        }
        .navigationSplitViewStyle(.balanced)
        .frame(minWidth: 720, minHeight: 500)
        .onAppear { useAdvancedMode = downloadManager.useAdvancedByDefault }
        .alert("Delete Download", isPresented: $showDeleteAlert, presenting: downloadToDelete) { dl in
            Button("Delete from List Only", role: .cancel) { downloadManager.deleteDownload(dl, deleteFile: false) }
            Button("Delete File Also", role: .destructive) { downloadManager.deleteDownload(dl, deleteFile: true) }
            Button("Cancel", role: .cancel) {}
        } message: { dl in
            Text("Delete '\(dl.filename)' from list, or also delete the downloaded file?")
        }
        .background(hiddenShortcuts)
    }

    // MARK: - Sidebar

    private var sidebarView: some View {
        List(SidebarItem.allCases, id: \.self, selection: $selectedSidebar) { item in
            HStack {
                Label(item.rawValue, systemImage: item.systemImage)
                Spacer()
                let count = downloadCount(for: item)
                if count > 0 {
                    Text("\(count)")
                        .font(.system(.caption2, design: .rounded))
                        .fontWeight(.semibold)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 7)
                        .padding(.vertical, 3)
                        .background(.secondary.opacity(0.15))
                        .clipShape(Capsule())
                }
            }
            .tag(item)
        }
        .listStyle(.sidebar)
        .navigationSplitViewColumnWidth(min: 160, ideal: 190, max: 240)
        .navigationTitle("KGet")
    }

    private func downloadCount(for item: SidebarItem) -> Int {
        switch item {
        case .all:       return downloadManager.downloads.count
        case .active:    return downloadManager.downloads.filter { $0.status == .downloading || $0.status == .verifying || $0.status == .pending }.count
        case .completed: return downloadManager.downloads.filter { $0.status == .completed }.count
        case .failed:    return downloadManager.downloads.filter { $0.status == .failed || $0.status == .cancelled }.count
        case .history:   return downloadManager.historyEntries.count
        }
    }

    // MARK: - Detail

    private var detailView: some View {
        VStack(spacing: 0) {
            if currentFilter == .history {
                historyView
            } else {
                downloadsView
            }
        }
        .background(Color(NSColor.windowBackgroundColor))
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                if currentFilter != .history,
                   downloadManager.downloads.filter({ $0.status == .completed }).count > 0 {
                    Button("Clear Completed") { downloadManager.clearCompleted() }
                }
            }
        }
        .sheet(isPresented: $downloadManager.showNewDownloadSheet) {
            NewDownloadSheet()
        }
        .onDrop(of: [UTType.url, UTType.plainText], isTargeted: $isDragTargeted) { providers in
            handleURLDrop(providers: providers)
        }
        .overlay {
            if isDragTargeted {
                ZStack {
                    Color.accentColor.opacity(0.08)
                    VStack(spacing: 12) {
                        Image(systemName: "arrow.down.circle.fill")
                            .font(.system(size: 52, weight: .light))
                        Text("Drop URL here")
                            .font(.title2).fontWeight(.semibold)
                    }
                    .foregroundStyle(Color.accentColor.opacity(0.75))
                }
                .allowsHitTesting(false)
                .transition(.opacity)
                .animation(.easeInOut(duration: 0.15), value: isDragTargeted)
            }
        }
        .onChange(of: selectedSidebar) { sidebar in
            if sidebar == .history { downloadManager.loadHistory() }
        }
        .animation(.easeInOut(duration: 0.25), value: downloadManager.clipboardDetectedURL)
    }

    // MARK: - Downloads (normal mode)

    private var downloadsView: some View {
        VStack(spacing: 0) {
            if let url = downloadManager.clipboardDetectedURL {
                clipboardBannerView(url: url)
                    .transition(.move(edge: .top).combined(with: .opacity))
            }

            urlInputBar

            if filteredDownloads.isEmpty {
                emptyState
            } else {
                downloadsList
            }

            if !downloadManager.downloads.isEmpty {
                footerBar
            }
        }
    }

    // MARK: - History View

    private var historyView: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Download History")
                    .font(.headline)
                Spacer()
                Button { downloadManager.loadHistory() } label: {
                    Image(systemName: "arrow.clockwise")
                        .font(.system(size: 12))
                }
                .buttonStyle(.plain)
                .help("Refresh history")
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .overlay(alignment: .bottom) { Divider().opacity(0.45) }

            if downloadManager.historyEntries.isEmpty {
                VStack(spacing: 16) {
                    Spacer()
                    ZStack {
                        Circle().fill(.ultraThinMaterial)
                            .overlay {
                                Circle().fill(LinearGradient(
                                    colors: [Color.white.opacity(0.18), Color.white.opacity(0.02)],
                                    startPoint: .topLeading, endPoint: .bottomTrailing
                                ))
                                Circle().strokeBorder(LinearGradient(
                                    colors: [Color.white.opacity(0.45), Color.white.opacity(0.08)],
                                    startPoint: .topLeading, endPoint: .bottomTrailing
                                ), lineWidth: 0.75)
                            }
                            .frame(width: 80, height: 80)
                        Image(systemName: "clock.arrow.circlepath")
                            .font(.system(size: 34, weight: .light))
                            .foregroundStyle(.secondary)
                    }
                    VStack(spacing: 6) {
                        Text("No History Yet")
                            .font(.title2).fontWeight(.semibold)
                        Text("Completed downloads will appear here")
                            .font(.callout).foregroundStyle(.secondary)
                    }
                    Spacer()
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(downloadManager.historyEntries) { entry in
                            HistoryRowView(entry: entry) {
                                _ = downloadManager.startDownload(url: entry.url)
                                selectedSidebar = .all
                            }
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                }
            }
        }
        .onAppear { downloadManager.loadHistory() }
    }

    // MARK: - Clipboard Banner

    @ViewBuilder
    private func clipboardBannerView(url: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "doc.on.clipboard.fill")
                .font(.system(size: 13))
                .foregroundStyle(Color.accentColor)

            VStack(alignment: .leading, spacing: 1) {
                Text("Link detected in clipboard")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(.secondary)
                Text(url)
                    .font(.system(size: 12))
                    .lineLimit(1)
                    .foregroundStyle(.primary)
            }

            Spacer()

            Button("Download") {
                withAnimation { downloadManager.dismissClipboardURL() }
                urlInput = url
                startDownload()
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.small)

            Button {
                withAnimation { downloadManager.dismissClipboardURL() }
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.tertiary)
                    .font(.system(size: 14))
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(Color.accentColor.opacity(0.07))
        .overlay(alignment: .bottom) { Divider().opacity(0.35) }
    }

    // MARK: - URL Input Bar (glass pill)

    private var urlInputBar: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {

                // Glass pill search field
                HStack(spacing: 10) {
                    Image(systemName: "arrow.down.circle.fill")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(
                            LinearGradient(
                                colors: [Color.accentColor, Color.accentColor.opacity(0.7)],
                                startPoint: .top, endPoint: .bottom
                            )
                        )

                    TextField("Paste a URL to download…", text: $urlInput)
                        .textFieldStyle(.plain)
                        .focused($urlFieldFocused)
                        .font(.system(size: 14))
                        .onSubmit { startDownload() }

                    if !urlInput.isEmpty {
                        Button { urlInput = "" } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.tertiary)
                                .font(.system(size: 14))
                        }
                        .buttonStyle(.plain)
                    }

                    Button(action: pasteAndDownload) {
                        Image(systemName: "doc.on.clipboard")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .help("Paste from clipboard and download")
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 9)
                .glassCard(cornerRadius: 22)

                // Turbo glass pill toggle
                HStack(spacing: 6) {
                    Toggle("", isOn: $useAdvancedMode)
                        .toggleStyle(.checkbox)
                        .help("Parallel chunk downloading")
                    Text("Turbo")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(useAdvancedMode ? Color.orange : Color.secondary)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .glassCard(cornerRadius: 12)

                // Download button
                Button(action: startDownload) {
                    Label("Download", systemImage: "arrow.down")
                        .fontWeight(.semibold)
                }
                .buttonStyle(.borderedProminent)
                .disabled(urlInput.isEmpty)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 11)

            Divider().opacity(0.45)
        }
        .background(Color(NSColor.windowBackgroundColor))
    }

    // MARK: - Downloads List

    private var downloadsList: some View {
        ScrollView {
            LazyVStack(spacing: 8) {
                ForEach(filteredDownloads) { download in
                    DownloadRowView(
                        download: download,
                        isSelected: downloadManager.selectedDownloadId == download.id,
                        onDelete: { dl in downloadToDelete = dl; showDeleteAlert = true },
                        onVerify: { dl in downloadManager.verifyISOChecksum(dl) }
                    )
                    .onTapGesture { downloadManager.selectedDownloadId = download.id }
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
        }
    }

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 18) {
            Spacer()

            // Floating glass icon
            ZStack {
                Circle()
                    .fill(.ultraThinMaterial)
                    .overlay {
                        Circle()
                            .fill(LinearGradient(
                                colors: [Color.white.opacity(0.22), Color.white.opacity(0.02)],
                                startPoint: .topLeading, endPoint: .bottomTrailing
                            ))
                        Circle()
                            .strokeBorder(
                                LinearGradient(
                                    colors: [Color.white.opacity(0.50), Color.white.opacity(0.08)],
                                    startPoint: .topLeading, endPoint: .bottomTrailing
                                ),
                                lineWidth: 0.75
                            )
                    }
                    .frame(width: 84, height: 84)
                    .shadow(color: Color.accentColor.opacity(0.10), radius: 20, y: 6)
                    .shadow(color: .black.opacity(0.08), radius: 8, y: 3)

                Image(systemName: currentFilter == .all ? "arrow.down.circle" : currentFilter.systemImage)
                    .font(.system(size: 38, weight: .light))
                    .foregroundStyle(
                        LinearGradient(
                            colors: [.primary.opacity(0.5), .primary.opacity(0.25)],
                            startPoint: .top, endPoint: .bottom
                        )
                    )
            }

            VStack(spacing: 7) {
                Text(currentFilter == .all ? "No Downloads" : "No \(currentFilter.rawValue)")
                    .font(.title2)
                    .fontWeight(.semibold)

                Text(currentFilter == .all
                     ? "Paste a URL above to start your first download"
                     : "Downloads will appear here when available")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }

            if currentFilter == .all {
                HStack(spacing: 10) {
                    glassChip(label: "HTTP/HTTPS", icon: "globe")
                    glassChip(label: "FTP/SFTP",   icon: "server.rack")
                    glassChip(label: "Magnets",     icon: "link")
                    glassChip(label: "Metalink",    icon: "list.bullet")
                }
                .padding(.top, 2)
            }

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func glassChip(label: String, icon: String) -> some View {
        VStack(spacing: 7) {
            Image(systemName: icon).font(.title3)
            Text(label).font(.caption2).fontWeight(.medium)
        }
        .foregroundStyle(.secondary)
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .glassCard(cornerRadius: 12)
    }

    // MARK: - Footer Bar

    private var footerBar: some View {
        HStack {
            let activeCount    = downloadManager.downloads.filter { $0.status == .downloading }.count
            let completedCount = downloadManager.downloads.filter { $0.status == .completed }.count

            if activeCount > 0 {
                Label("\(activeCount) downloading", systemImage: "arrow.down")
                    .foregroundStyle(Color.accentColor)
            }
            Label("\(completedCount) completed", systemImage: "checkmark")
                .foregroundStyle(completedCount > 0 ? Color.green : Color.secondary)

            Spacer()

            if completedCount > 0 {
                Button("Clear Completed") { downloadManager.clearCompleted() }
                    .buttonStyle(.link)
            }
        }
        .font(.caption)
        .padding(.horizontal, 16)
        .padding(.vertical, 9)
        .background(Color(NSColor.windowBackgroundColor))
        .overlay(alignment: .top) { Divider().opacity(0.45) }
    }

    // MARK: - Keyboard Shortcuts

    @ViewBuilder
    private var hiddenShortcuts: some View {
        Group {
            Button("") { pasteAndDownload() }.keyboardShortcut("v", modifiers: .command)
            Button("") { urlFieldFocused = true }.keyboardShortcut("l", modifiers: .command)
            Button("") { downloadManager.cancelSelectedDownload() }.keyboardShortcut(.escape, modifiers: [])
        }
        .opacity(0).frame(width: 0, height: 0)
    }

    // MARK: - Actions

    private func startDownload() {
        guard !urlInput.isEmpty else { return }
        if downloadManager.startDownload(url: urlInput, advanced: useAdvancedMode) { urlInput = "" }
    }

    private func pasteAndDownload() {
        guard let string = NSPasteboard.general.string(forType: .string) else { return }
        urlInput = string
        startDownload()
    }

    private func handleURLDrop(providers: [NSItemProvider]) -> Bool {
        for provider in providers {
            if provider.hasItemConformingToTypeIdentifier(UTType.url.identifier) {
                _ = provider.loadObject(ofClass: URL.self) { url, _ in
                    guard let url = url, !url.isFileURL else { return }
                    DispatchQueue.main.async { self.urlInput = url.absoluteString }
                }
                return true
            }
            if provider.hasItemConformingToTypeIdentifier(UTType.plainText.identifier) {
                _ = provider.loadObject(ofClass: String.self) { string, _ in
                    guard let string = string else { return }
                    let trimmed = string.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard self.downloadManager.isDownloadableURL(trimmed) else { return }
                    DispatchQueue.main.async { self.urlInput = trimmed }
                }
                return true
            }
        }
        return false
    }
}

// MARK: - History Row

struct HistoryRowView: View {
    let entry: HistoryEntry
    let onRedownload: () -> Void
    @State private var isHovered = false

    private static let dateFormatter: DateFormatter = {
        let df = DateFormatter()
        df.dateStyle = .medium
        df.timeStyle = .short
        return df
    }()

    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(entry.statusColor.opacity(0.18))
                    .frame(width: 34, height: 34)
                Image(systemName: entry.statusIcon)
                    .font(.system(size: 15))
                    .foregroundStyle(entry.statusColor)
            }

            VStack(alignment: .leading, spacing: 3) {
                Text(entry.filename)
                    .font(.system(size: 13, weight: .semibold))
                    .lineLimit(1)
                Text(entry.url)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()

            VStack(alignment: .trailing, spacing: 3) {
                Text(Self.dateFormatter.string(from: entry.createdAtDate))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                if let size = entry.bytesFormatted {
                    Text(size)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }

            if isHovered {
                Button(action: onRedownload) {
                    Image(systemName: "arrow.down.circle.fill")
                        .font(.system(size: 17))
                        .foregroundStyle(Color.accentColor)
                }
                .buttonStyle(.plain)
                .help("Re-download")
                .transition(.opacity.combined(with: .scale))
            }
        }
        .padding(12)
        .glassCard(cornerRadius: 10, isHovered: isHovered)
        .onHover { isHovered = $0 }
        .animation(.easeInOut(duration: 0.15), value: isHovered)
    }
}

// MARK: - Download Row

struct DownloadRowView: View {
    @EnvironmentObject var downloadManager: DownloadManager
    let download: Download
    let isSelected: Bool
    let onDelete: (Download) -> Void
    let onVerify: (Download) -> Void
    @State private var isHovered = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .center, spacing: 8) {
                statusIndicator

                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 5) {
                        Text(download.filename)
                            .font(.system(size: 13, weight: .semibold))
                            .lineLimit(1)
                        if download.isAdvanced { TypeBadge(text: "Turbo",   color: .orange) }
                        if download.isISO      { TypeBadge(text: "ISO",     color: .purple) }
                        if download.isTorrent  { TypeBadge(text: "Torrent", color: .green)  }
                    }
                    Text(download.url).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }

                Spacer()
                actionButtons
            }

            if download.status == .downloading || download.status == .verifying {
                let pv    = download.status == .verifying ? download.verificationProgress / 100.0 : download.progress / 100.0
                let pcolor: Color = download.status == .verifying ? .purple : .accentColor

                LiquidProgressBar(progress: pv, color: pcolor).frame(height: 5)

                HStack {
                    Text("\(Int(pv * 100))%")
                        .font(.system(.caption, design: .monospaced))
                        .fontWeight(.semibold)
                        .foregroundStyle(pcolor)
                    Spacer()
                    if download.status == .verifying {
                        Label("Calculating SHA256…", systemImage: "checkmark.shield")
                            .font(.caption).foregroundStyle(.purple)
                    } else {
                        downloadStatsRow
                    }
                }
            }

            if download.isTorrent && !download.torrentFiles.isEmpty { torrentFilesSection }
            if download.status == .completed { completedInfoRow }

            if let error = download.error {
                HStack(spacing: 4) {
                    Image(systemName: "exclamationmark.triangle.fill").font(.caption2)
                    Text(error).font(.caption).lineLimit(2)
                }
                .foregroundStyle(.red)
            }
        }
        .padding(13)
        .glassCard(cornerRadius: 12, isSelected: isSelected, isHovered: isHovered)
        .animation(.easeInOut(duration: 0.15), value: isHovered)
        .animation(.easeInOut(duration: 0.15), value: isSelected)
        .contextMenu { contextMenuContent }
        .onHover { isHovered = $0 }
    }

    private var statusIndicator: some View {
        ZStack {
            Circle().fill(statusColor.opacity(0.22)).frame(width: 14, height: 14)
            Circle().fill(statusColor).frame(width: 8, height: 8)
        }
        .shadow(color: statusColor.opacity(download.status == .downloading ? 0.6 : 0.0), radius: 4)
    }

    private var statusColor: Color {
        switch download.status {
        case .pending:     return .secondary
        case .downloading: return .accentColor
        case .verifying:   return .purple
        case .completed:   return .green
        case .failed:      return .red
        case .cancelled:   return .orange
        }
    }

    private var actionButtons: some View {
        HStack(spacing: 4) {
            Button { downloadManager.copyURL(download) } label: {
                Image(systemName: "link").foregroundStyle(.secondary)
            }.buttonStyle(.plain).help("Copy URL")

            if download.status == .downloading {
                Button { downloadManager.cancelDownload(download) } label: {
                    Image(systemName: "stop.circle").foregroundStyle(.red)
                }.buttonStyle(.plain).help("Cancel")
            }

            if download.status == .failed || download.status == .cancelled {
                Button { downloadManager.retryDownload(download) } label: {
                    Image(systemName: "arrow.clockwise").foregroundStyle(Color.accentColor)
                }.buttonStyle(.plain).help("Retry")
            }

            if download.status == .completed && download.isISO && download.sha256Checksum == nil {
                Button { onVerify(download) } label: {
                    Image(systemName: "checkmark.shield").foregroundStyle(.purple)
                }.buttonStyle(.plain).help("Verify SHA256")
            }

            if download.sha256Checksum != nil {
                Button { downloadManager.copySHA256(download) } label: {
                    Image(systemName: "number").foregroundStyle(.green)
                }.buttonStyle(.plain).help("Copy SHA256")
            }

            if download.status == .completed {
                Button { downloadManager.openFile(download) } label: {
                    Image(systemName: "doc").foregroundStyle(.secondary)
                }.buttonStyle(.plain).help("Open file")
                Button { downloadManager.openFolder(download) } label: {
                    Image(systemName: "folder").foregroundStyle(.secondary)
                }.buttonStyle(.plain).help("Show in Finder")
            }

            Button { onDelete(download) } label: {
                Image(systemName: "trash").foregroundStyle(.red.opacity(0.7))
            }.buttonStyle(.plain).help("Delete")
        }
        .font(.system(size: 13))
    }

    private var downloadStatsRow: some View {
        HStack(spacing: 10) {
            if !download.downloadedSize.isEmpty || !download.totalSize.isEmpty {
                Text("\(download.downloadedSize) / \(download.totalSize)").foregroundStyle(.secondary)
            }
            if !download.speed.isEmpty {
                HStack(spacing: 2) {
                    Image(systemName: "arrow.down").font(.system(size: 9))
                    Text(download.speed).monospacedDigit()
                }.foregroundStyle(Color.accentColor)
            }
            if download.speedHistory.count > 1 {
                SparklineView(samples: download.speedHistory)
                    .frame(width: 44, height: 16)
            }
            if !download.eta.isEmpty {
                HStack(spacing: 2) {
                    Image(systemName: "clock").font(.system(size: 9))
                    Text(download.eta).monospacedDigit()
                }.foregroundStyle(.secondary)
            }
            if download.isAdvanced && download.activeConnections > 1 {
                HStack(spacing: 2) {
                    Image(systemName: "arrow.triangle.branch").font(.system(size: 9))
                    Text("\(download.activeConnections)x").monospacedDigit()
                }.foregroundStyle(.orange).help("Parallel connections")
            }
        }
        .font(.caption)
    }

    private var torrentFilesSection: some View {
        DisclosureGroup(
            isExpanded: Binding(
                get: { download.isExpanded },
                set: { newVal in
                    if let idx = downloadManager.downloads.firstIndex(where: { $0.id == download.id }) {
                        withAnimation(.easeInOut(duration: 0.18)) { downloadManager.downloads[idx].isExpanded = newVal }
                    }
                }
            )
        ) {
            VStack(spacing: 5) {
                ForEach(download.torrentFiles) { file in
                    HStack(spacing: 8) {
                        Image(systemName: "doc").font(.caption2).foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(file.name).font(.caption).lineLimit(1)
                            LiquidProgressBar(progress: file.progress / 100.0, color: .green).frame(height: 2)
                        }
                        Text("\(Int(file.progress))%")
                            .font(.system(.caption2, design: .monospaced))
                            .foregroundStyle(.secondary).frame(width: 34, alignment: .trailing)
                        Text(file.sizeFormatted)
                            .font(.caption2).foregroundStyle(.secondary).frame(width: 58, alignment: .trailing)
                    }
                }
            }
            .padding(.leading, 4).padding(.top, 4)
        } label: {
            Text("\(download.torrentFiles.count) file(s)").font(.caption).foregroundStyle(.secondary)
        }
        .buttonStyle(.plain)
    }

    private var completedInfoRow: some View {
        HStack {
            if let size = download.fileSize {
                Label(size, systemImage: "doc").font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            if let checksum = download.sha256Checksum {
                HStack(spacing: 4) {
                    Image(systemName: "checkmark.seal.fill").font(.caption2).foregroundStyle(.green)
                    Text("SHA256: \(String(checksum.prefix(12)))…")
                        .font(.system(.caption2, design: .monospaced)).foregroundStyle(.secondary)
                }.help("Full SHA256: \(checksum)")
            }
            Label("Completed", systemImage: "checkmark.circle.fill").font(.caption).foregroundStyle(.green)
        }
    }

    @ViewBuilder
    private var contextMenuContent: some View {
        Button("Copy URL") { downloadManager.copyURL(download) }
        if download.status == .completed {
            Button("Open File")   { downloadManager.openFile(download) }
            Button("Open Folder") { downloadManager.openFolder(download) }
        }
        if download.sha256Checksum != nil { Button("Copy SHA256") { downloadManager.copySHA256(download) } }
        if download.status == .failed || download.status == .cancelled || download.status == .completed {
            Button("Restart") { downloadManager.retryDownload(download) }
        }
        Divider()
        Button("Remove", role: .destructive) { onDelete(download) }
    }
}

// MARK: - Type Badge

struct TypeBadge: View {
    let text: String
    let color: Color

    var body: some View {
        Text(text.uppercased())
            .font(.system(size: 8, weight: .bold)).tracking(0.3)
            .padding(.horizontal, 5).padding(.vertical, 2)
            .foregroundStyle(color)
            .background(color.opacity(0.15))
            .clipShape(RoundedRectangle(cornerRadius: 3))
            .overlay { RoundedRectangle(cornerRadius: 3).strokeBorder(color.opacity(0.25), lineWidth: 0.5) }
    }
}

// MARK: - Liquid Progress Bar

struct LiquidProgressBar: View {
    let progress: Double
    let color: Color
    @State private var shimmerPhase: CGFloat = 0

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                // Track
                RoundedRectangle(cornerRadius: 3)
                    .fill(color.opacity(0.12))
                    .overlay { RoundedRectangle(cornerRadius: 3).strokeBorder(color.opacity(0.10), lineWidth: 0.5) }

                // Liquid fill
                let fw = geo.size.width * CGFloat(min(max(progress, 0), 1))
                RoundedRectangle(cornerRadius: 3)
                    .fill(LinearGradient(colors: [color.opacity(0.9), color], startPoint: .leading, endPoint: .trailing))
                    .frame(width: fw)
                    .animation(.easeInOut(duration: 0.3), value: progress)
                    .overlay(alignment: .top) {
                        RoundedRectangle(cornerRadius: 3)
                            .fill(LinearGradient(colors: [.white.opacity(0.45), .white.opacity(0)], startPoint: .top, endPoint: .bottom))
                            .frame(height: geo.size.height / 2)
                    }
                    .overlay(
                        Rectangle()
                            .fill(LinearGradient(colors: [.clear, .white.opacity(0.55), .clear], startPoint: .leading, endPoint: .trailing))
                            .frame(width: 28)
                            .offset(x: -fw + shimmerPhase * (fw + 28))
                            .clipped()
                    )
                    .clipShape(RoundedRectangle(cornerRadius: 3))
                    .shadow(color: color.opacity(0.45), radius: 4, y: 1)
            }
        }
        .onAppear {
            withAnimation(.linear(duration: 1.6).repeatForever(autoreverses: false)) { shimmerPhase = 1 }
        }
    }
}

// MARK: - Logo View

struct LogoView: View {
    var body: some View {
        Group {
            if let resourcePath = Bundle.main.resourcePath,
               let nsImage = NSImage(contentsOfFile: resourcePath + "/logo.png") {
                Image(nsImage: nsImage).resizable().aspectRatio(contentMode: .fit)
            } else if let bundlePath = Bundle.main.executableURL?
                .deletingLastPathComponent().deletingLastPathComponent()
                .appendingPathComponent("Resources/logo.png").path,
               let nsImage = NSImage(contentsOfFile: bundlePath) {
                Image(nsImage: nsImage).resizable().aspectRatio(contentMode: .fit)
            } else if FileManager.default.fileExists(atPath: "logo.png"),
                      let nsImage = NSImage(contentsOfFile: "logo.png") {
                Image(nsImage: nsImage).resizable().aspectRatio(contentMode: .fit)
            } else {
                ZStack {
                    RoundedRectangle(cornerRadius: 18)
                        .fill(.ultraThinMaterial)
                        .overlay {
                            RoundedRectangle(cornerRadius: 18)
                                .fill(LinearGradient(colors: [.blue.opacity(0.8), .purple.opacity(0.8)], startPoint: .topLeading, endPoint: .bottomTrailing))
                            RoundedRectangle(cornerRadius: 18)
                                .fill(LinearGradient(colors: [.white.opacity(0.25), .white.opacity(0.02)], startPoint: .topLeading, endPoint: .bottom))
                        }
                        .overlay {
                            RoundedRectangle(cornerRadius: 18)
                                .strokeBorder(LinearGradient(colors: [.white.opacity(0.5), .white.opacity(0.1)], startPoint: .topLeading, endPoint: .bottomTrailing), lineWidth: 0.75)
                        }
                    Image(systemName: "arrow.down.circle.fill").font(.system(size: 28)).foregroundStyle(.white)
                        .shadow(color: .black.opacity(0.2), radius: 4, y: 2)
                }
            }
        }
    }
}

// MARK: - SparklineView

struct SparklineView: View {
    let samples: [Double]
    var color: Color = .accentColor

    var body: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let h = geo.size.height
            let max = samples.max() ?? 1
            let min = samples.min() ?? 0
            let range = max - min > 0 ? max - min : 1
            let points = samples.enumerated().map { i, v -> CGPoint in
                let x = w * CGFloat(i) / CGFloat(samples.count - 1)
                let y = h - h * CGFloat(v - min) / CGFloat(range)
                return CGPoint(x: x, y: y)
            }

            ZStack {
                if points.count > 1 {
                    Path { p in
                        p.move(to: CGPoint(x: points[0].x, y: h))
                        p.addLine(to: points[0])
                        for pt in points.dropFirst() { p.addLine(to: pt) }
                        p.addLine(to: CGPoint(x: points.last!.x, y: h))
                        p.closeSubpath()
                    }
                    .fill(LinearGradient(
                        colors: [color.opacity(0.3), color.opacity(0.05)],
                        startPoint: .top, endPoint: .bottom
                    ))

                    Path { p in
                        p.move(to: points[0])
                        for pt in points.dropFirst() { p.addLine(to: pt) }
                    }
                    .stroke(color, lineWidth: 1.5)
                }
            }
        }
    }
}

#if DEBUG
#Preview {
    ContentView().environmentObject(DownloadManager.shared)
}
#endif
