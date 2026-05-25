//
//  SettingsView.swift
//  KGet
//
//  Settings — liquid glass sections + About page redesign
//

import SwiftUI

// MARK: - Settings View

struct SettingsView: View {
    @EnvironmentObject var downloadManager: DownloadManager
    @State private var downloadPath: String = ""
    @State private var showNotifications: Bool = true
    @State private var speedLimit: String = ""
    @State private var useAdvancedByDefault: Bool = false
    @State private var verifyISOIntegrity: Bool = true
    @State private var autoExtract: Bool = false
    @State private var videoQuality: String = "best"
    private let videoQualities = ["best", "1080p", "720p", "480p", "360p", "audio"]

    var body: some View {
        TabView {
            generalTab
                .tabItem { Label("General", systemImage: "gear") }

            downloadsTab
                .tabItem { Label("Downloads", systemImage: "arrow.down.circle") }

            aboutTab
                .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 540, height: 380)
        .onAppear {
            downloadPath    = downloadManager.defaultDownloadPath.path
            verifyISOIntegrity  = downloadManager.verifyISOIntegrity
            showNotifications   = downloadManager.showNotifications
            useAdvancedByDefault = downloadManager.useAdvancedByDefault
            speedLimit          = downloadManager.speedLimitKBPerSecond
            autoExtract         = downloadManager.autoExtract
            videoQuality        = downloadManager.videoQuality
        }
        .onChange(of: verifyISOIntegrity)   { v in downloadManager.verifyISOIntegrity = v;      downloadManager.saveSettings() }
        .onChange(of: showNotifications)    { v in downloadManager.showNotifications = v;        downloadManager.saveSettings() }
        .onChange(of: useAdvancedByDefault) { v in downloadManager.useAdvancedByDefault = v;    downloadManager.saveSettings() }
        .onChange(of: speedLimit)           { v in downloadManager.speedLimitKBPerSecond = v;   downloadManager.saveSettings() }
        .onChange(of: autoExtract)          { v in downloadManager.autoExtract = v;             downloadManager.saveSettings() }
        .onChange(of: videoQuality)         { v in downloadManager.videoQuality = v;            downloadManager.saveSettings() }
    }

    // MARK: - General Tab

    private var generalTab: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {

                // Download Location section
                GlassSection(header: "Storage") {
                    GlassRow(icon: "folder.fill", iconColor: .blue, title: "Download Location") {
                        HStack(spacing: 8) {
                            Text(downloadPath)
                                .font(.system(size: 12))
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                                .frame(maxWidth: 200, alignment: .trailing)
                            Button("Choose…") { selectDownloadFolder() }
                                .buttonStyle(.bordered)
                                .controlSize(.small)
                        }
                    }
                }

                // Notifications section
                GlassSection(header: "Notifications") {
                    GlassRow(
                        icon: "bell.badge.fill",
                        iconColor: .red,
                        title: "Show on completion",
                        subtitle: "Get a notification when each download finishes"
                    ) {
                        Toggle("", isOn: $showNotifications).labelsHidden()
                    }
                }

                Spacer()
            }
            .padding(20)
        }
    }

    // MARK: - Downloads Tab

    private var downloadsTab: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {

                // Performance section
                GlassSection(header: "Performance") {
                    GlassRow(
                        icon: "bolt.fill",
                        iconColor: .orange,
                        title: "Turbo mode by default",
                        subtitle: "Download files in parallel chunks for maximum speed"
                    ) {
                        Toggle("", isOn: $useAdvancedByDefault).labelsHidden()
                    }

                    GlassDivider()

                    GlassRow(icon: "speedometer", iconColor: .purple, title: "Speed limit (KB/s)") {
                        TextField("0 = unlimited", text: $speedLimit)
                            .textFieldStyle(.roundedBorder)
                            .frame(width: 110)
                            .font(.system(size: 12, design: .monospaced))
                    }
                }

                // ISO Files section
                GlassSection(header: "ISO Files") {
                    GlassRow(
                        icon: "checkmark.shield.fill",
                        iconColor: .green,
                        title: "Auto-verify integrity",
                        subtitle: "Calculate SHA256 checksum after downloading .iso files"
                    ) {
                        Toggle("", isOn: $verifyISOIntegrity).labelsHidden()
                    }
                }

                // Video section
                GlassSection(header: "Video Downloads (yt-dlp)") {
                    GlassRow(icon: "play.rectangle.fill", iconColor: .red, title: "Default quality",
                             subtitle: "Used when yt-dlp downloads YouTube, Vimeo, etc.") {
                        Picker("", selection: $videoQuality) {
                            Text("Best available").tag("best")
                            Text("1080p").tag("1080p")
                            Text("720p").tag("720p")
                            Text("480p").tag("480p")
                            Text("360p").tag("360p")
                            Text("Audio only").tag("audio")
                        }
                        .labelsHidden()
                        .frame(width: 140)
                        .pickerStyle(.menu)
                    }
                }

                // Archives section
                GlassSection(header: "Archives") {
                    GlassRow(
                        icon: "archivebox.fill",
                        iconColor: .indigo,
                        title: "Auto-extract archives",
                        subtitle: "Unzip/untar .zip, .tar.gz, .7z files after download"
                    ) {
                        Toggle("", isOn: $autoExtract).labelsHidden()
                    }
                }

                Spacer()
            }
            .padding(20)
        }
    }

    // MARK: - About Tab

    private var aboutTab: some View {
        ZStack {
            // Gradient backdrop
            LinearGradient(
                colors: [
                    Color.accentColor.opacity(0.06),
                    Color.purple.opacity(0.04),
                    Color.clear
                ],
                startPoint: .topLeading, endPoint: .bottomTrailing
            )
            .ignoresSafeArea()

            VStack(spacing: 0) {
                Spacer(minLength: 18)

                // Floating glass icon
                ZStack {
                    // Outer glass disc
                    Circle()
                        .fill(.ultraThinMaterial)
                        .overlay {
                            Circle().fill(LinearGradient(
                                colors: [.white.opacity(0.22), .white.opacity(0.02)],
                                startPoint: .topLeading, endPoint: .bottomTrailing
                            ))
                            Circle().strokeBorder(
                                LinearGradient(
                                    colors: [.white.opacity(0.48), .white.opacity(0.08)],
                                    startPoint: .topLeading, endPoint: .bottomTrailing
                                ),
                                lineWidth: 0.75
                            )
                        }
                        .frame(width: 90, height: 90)
                        .shadow(color: Color.accentColor.opacity(0.15), radius: 18, y: 6)
                        .shadow(color: .black.opacity(0.12), radius: 6, y: 3)

                    LogoView().frame(width: 62, height: 62)
                }

                Spacer(minLength: 14)

                // App name + version
                VStack(spacing: 6) {
                    Text("KGet")
                        .font(.system(size: 24, weight: .bold))

                    // Version badge
                    let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "?"
                    Text("Version \(version)")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 4)
                        .background(.secondary.opacity(0.10))
                        .clipShape(Capsule())
                        .overlay { Capsule().strokeBorder(Color.secondary.opacity(0.20), lineWidth: 0.5) }
                }

                Spacer(minLength: 10)

                Text("A fast, modern download manager built with Rust and SwiftUI")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 30)

                Spacer(minLength: 14)

                // Glass link buttons
                VStack(spacing: 7) {
                    aboutLink(label: "GitHub Repository",  icon: "chevron.left.forwardslash.chevron.right", dest: "https://github.com/davimf721/KGet",        color: .primary)
                    aboutLink(label: "Report an Issue",    icon: "exclamationmark.bubble",                   dest: "https://github.com/davimf721/KGet/issues", color: .orange)
                    aboutLink(label: "crates.io",          icon: "shippingbox.fill",                         dest: "https://crates.io/crates/Kget",            color: .accentColor)
                }
                .padding(.horizontal, 30)

                Spacer(minLength: 12)

                Text("© 2026 Davi Moreira Fuzatto")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)

                Spacer(minLength: 10)
            }
        }
    }

    @ViewBuilder
    private func aboutLink(label: String, icon: String, dest: String, color: Color) -> some View {
        if let url = URL(string: dest) {
            Link(destination: url) {
                HStack(spacing: 10) {
                    ZStack {
                        RoundedRectangle(cornerRadius: 7)
                            .fill(color.opacity(0.12))
                            .frame(width: 28, height: 28)
                        Image(systemName: icon)
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(color)
                    }
                    Text(label)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(.primary)
                    Spacer()
                    Image(systemName: "arrow.up.right")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background {
                    ZStack {
                        RoundedRectangle(cornerRadius: 10).fill(.ultraThinMaterial)
                        RoundedRectangle(cornerRadius: 10)
                            .fill(LinearGradient(colors: [.white.opacity(0.10), .white.opacity(0.02)], startPoint: .topLeading, endPoint: .bottomTrailing))
                    }
                }
                .overlay {
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(
                            LinearGradient(colors: [.white.opacity(0.30), .white.opacity(0.07)], startPoint: .topLeading, endPoint: .bottomTrailing),
                            lineWidth: 0.75
                        )
                }
                .shadow(color: .black.opacity(0.05), radius: 4, y: 2)
            }
            .buttonStyle(.plain)
        }
    }

    // MARK: - Helpers

    private func selectDownloadFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Select"
        if panel.runModal() == .OK, let url = panel.url {
            downloadPath = url.path
            downloadManager.defaultDownloadPath = url
            downloadManager.saveSettings()
        }
    }
}

// MARK: - Glass Section

struct GlassSection<Content: View>: View {
    let header: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(header.uppercased())
                .font(.system(size: 10, weight: .semibold))
                .tracking(0.5)
                .foregroundStyle(.secondary)
                .padding(.leading, 4)

            VStack(spacing: 0) {
                content
            }
            .background {
                ZStack {
                    RoundedRectangle(cornerRadius: 12).fill(.ultraThinMaterial)
                    RoundedRectangle(cornerRadius: 12)
                        .fill(LinearGradient(colors: [.white.opacity(0.10), .white.opacity(0.02)], startPoint: .topLeading, endPoint: .bottomTrailing))
                }
            }
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .strokeBorder(
                        LinearGradient(colors: [.white.opacity(0.32), .white.opacity(0.07)], startPoint: .topLeading, endPoint: .bottomTrailing),
                        lineWidth: 0.75
                    )
            }
            .shadow(color: .black.opacity(0.06), radius: 6, y: 2)
        }
    }
}

// MARK: - Glass Row

struct GlassRow<Trailing: View>: View {
    let icon: String
    let iconColor: Color
    let title: String
    var subtitle: String? = nil
    @ViewBuilder let trailing: Trailing

    var body: some View {
        HStack(spacing: 12) {
            // Icon chip
            ZStack {
                RoundedRectangle(cornerRadius: 7)
                    .fill(iconColor.opacity(0.15))
                    .frame(width: 32, height: 32)
                    .overlay {
                        RoundedRectangle(cornerRadius: 7)
                            .fill(LinearGradient(colors: [.white.opacity(0.18), .white.opacity(0.02)], startPoint: .topLeading, endPoint: .bottomTrailing))
                    }
                    .overlay {
                        RoundedRectangle(cornerRadius: 7)
                            .strokeBorder(iconColor.opacity(0.20), lineWidth: 0.5)
                    }
                Image(systemName: icon)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(iconColor)
            }

            // Labels
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 13, weight: .medium))
                if let sub = subtitle {
                    Text(sub)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }

            Spacer()
            trailing
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
    }
}

// MARK: - Glass Divider

struct GlassDivider: View {
    var body: some View {
        Divider()
            .padding(.leading, 58)
            .opacity(0.4)
    }
}

// MARK: - New Download Sheet

struct NewDownloadSheet: View {
    @EnvironmentObject var downloadManager: DownloadManager
    @Environment(\.dismiss) var dismiss

    @State private var url = ""
    @State private var outputPath = ""
    @State private var useAdvancedMode = false
    @State private var verifyChecksum = false
    @State private var expectedSHA256 = ""
    @State private var validationError: String?

    var isISO: Bool { url.lowercased().hasSuffix(".iso") }

    var body: some View {
        VStack(spacing: 20) {
            // Header
            HStack(spacing: 12) {
                ZStack {
                    RoundedRectangle(cornerRadius: 9)
                        .fill(.ultraThinMaterial)
                        .overlay {
                            RoundedRectangle(cornerRadius: 9)
                                .fill(LinearGradient(colors: [.white.opacity(0.18), .white.opacity(0.03)], startPoint: .topLeading, endPoint: .bottomTrailing))
                        }
                        .overlay {
                            RoundedRectangle(cornerRadius: 9)
                                .strokeBorder(LinearGradient(colors: [.white.opacity(0.38), .white.opacity(0.08)], startPoint: .topLeading, endPoint: .bottomTrailing), lineWidth: 0.75)
                        }
                        .frame(width: 34, height: 34)
                    LogoView().frame(width: 24, height: 24)
                }
                Text("New Download")
                    .font(.headline)
                Spacer()
            }

            // Fields
            VStack(spacing: 12) {
                GlassSection(header: "URL") {
                    HStack(spacing: 10) {
                        Image(systemName: "link").foregroundStyle(Color.accentColor).font(.system(size: 14))
                        TextField("https://…", text: $url).textFieldStyle(.plain).font(.system(size: 13))
                    }
                    .padding(.horizontal, 14).padding(.vertical, 10)
                }

                GlassSection(header: "Save to") {
                    HStack(spacing: 10) {
                        Image(systemName: "folder").foregroundStyle(Color.orange).font(.system(size: 14))
                        Text(outputPath).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                        Spacer()
                        Button("Browse…") { selectOutputPath() }.buttonStyle(.bordered).controlSize(.small)
                    }
                    .padding(.horizontal, 14).padding(.vertical, 10)
                }

                GlassSection(header: "Options") {
                    GlassRow(icon: "bolt.fill", iconColor: .orange, title: "Turbo mode",
                             subtitle: "Download in parallel chunks") {
                        Toggle("", isOn: $useAdvancedMode).labelsHidden()
                    }

                    if isISO {
                        GlassDivider()
                        GlassRow(icon: "checkmark.shield.fill", iconColor: .green, title: "Verify checksum") {
                            Toggle("", isOn: $verifyChecksum).labelsHidden()
                        }
                        if verifyChecksum {
                            GlassDivider()
                            HStack(spacing: 10) {
                                Image(systemName: "number").foregroundStyle(Color.purple).font(.system(size: 14))
                                TextField("Expected SHA256 (optional)", text: $expectedSHA256)
                                    .textFieldStyle(.plain).font(.system(size: 12, design: .monospaced))
                            }
                            .padding(.horizontal, 14).padding(.vertical, 10)
                        }
                    }
                }
            }

            if let err = validationError ?? downloadManager.lastStartError {
                HStack(spacing: 6) {
                    Image(systemName: "exclamationmark.triangle.fill")
                    Text(err)
                }
                .font(.caption).foregroundStyle(.red)
            }

            // Buttons
            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.escape)
                    .buttonStyle(GlassButtonStyle())
                Spacer()
                Button("Download") { startDownload() }
                    .keyboardShortcut(.return)
                    .buttonStyle(.borderedProminent)
                    .disabled(url.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 460)
        .onAppear {
            outputPath   = downloadManager.defaultDownloadPath.path
            verifyChecksum = downloadManager.verifyISOIntegrity
            useAdvancedMode = downloadManager.useAdvancedByDefault
            if let clip = NSPasteboard.general.string(forType: .string),
               clip.hasPrefix("http") || clip.hasPrefix("magnet:") || clip.hasPrefix("ftp:") {
                url = clip
            }
        }
    }

    private func selectOutputPath() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Select"
        if panel.runModal() == .OK, let u = panel.url { outputPath = u.path }
    }

    private func startDownload() {
        let ok = downloadManager.startDownload(
            url: url, outputPath: outputPath,
            advanced: useAdvancedMode, verifyISO: verifyChecksum,
            expectedSHA256: expectedSHA256
        )
        if ok { dismiss() } else { validationError = downloadManager.lastStartError }
    }
}

#if DEBUG
#Preview {
    SettingsView().environmentObject(DownloadManager.shared)
}
#endif
