//
//  MenuBarView.swift
//  KGet
//
//  Menu bar popover — full liquid glass redesign
//

import SwiftUI

struct MenuBarView: View {
    @EnvironmentObject var downloadManager: DownloadManager
    @State private var quickURL = ""
    @FocusState private var inputFocused: Bool

    private var activeDownloads: [Download] {
        Array(downloadManager.downloads.filter { $0.status == .downloading }.prefix(4))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            headerSection
            Divider().opacity(0.35)
            quickInputSection
            Divider().opacity(0.35)
            downloadsSection
            Divider().opacity(0.35)
            footerSection
        }
        .frame(width: 340)
        .background(.ultraThinMaterial)
    }

    // MARK: - Header

    private var headerSection: some View {
        HStack(spacing: 12) {
            // App icon in glass container
            ZStack {
                RoundedRectangle(cornerRadius: 10)
                    .fill(.ultraThinMaterial)
                    .overlay {
                        RoundedRectangle(cornerRadius: 10)
                            .fill(LinearGradient(
                                colors: [.white.opacity(0.20), .white.opacity(0.04)],
                                startPoint: .topLeading, endPoint: .bottomTrailing
                            ))
                    }
                    .overlay {
                        RoundedRectangle(cornerRadius: 10)
                            .strokeBorder(
                                LinearGradient(
                                    colors: [.white.opacity(0.40), .white.opacity(0.08)],
                                    startPoint: .topLeading, endPoint: .bottomTrailing
                                ),
                                lineWidth: 0.75
                            )
                    }
                    .frame(width: 36, height: 36)
                    .shadow(color: .black.opacity(0.12), radius: 6, y: 2)

                LogoView().frame(width: 26, height: 26)
            }

            VStack(alignment: .leading, spacing: 1) {
                Text("KGet")
                    .font(.system(size: 14, weight: .semibold))
                Text("Download Manager")
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }

            Spacer()

            // Active badge
            let activeCount = activeDownloads.count
            if activeCount > 0 {
                HStack(spacing: 4) {
                    Circle()
                        .fill(Color.accentColor)
                        .frame(width: 6, height: 6)
                        .shadow(color: .accentColor.opacity(0.7), radius: 3)
                    Text("\(activeCount)")
                        .font(.system(size: 11, weight: .semibold, design: .rounded))
                        .foregroundStyle(Color.accentColor)
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.accentColor.opacity(0.12))
                .clipShape(Capsule())
                .overlay { Capsule().strokeBorder(Color.accentColor.opacity(0.25), lineWidth: 0.5) }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
    }

    // MARK: - Quick Input

    private var quickInputSection: some View {
        HStack(spacing: 8) {
            // Glass pill input
            HStack(spacing: 8) {
                Image(systemName: "arrow.down.circle.fill")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(
                        LinearGradient(
                            colors: [Color.accentColor, Color.accentColor.opacity(0.7)],
                            startPoint: .top, endPoint: .bottom
                        )
                    )

                TextField("Quick download URL…", text: $quickURL)
                    .textFieldStyle(.plain)
                    .focused($inputFocused)
                    .font(.system(size: 13))
                    .onSubmit {
                        guard !quickURL.isEmpty else { return }
                        downloadManager.startDownload(url: quickURL)
                        quickURL = ""
                    }

                if !quickURL.isEmpty {
                    Button { quickURL = "" } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.tertiary)
                    }.buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 11)
            .padding(.vertical, 8)
            .background {
                ZStack {
                    RoundedRectangle(cornerRadius: 18)
                        .fill(.ultraThinMaterial)
                    RoundedRectangle(cornerRadius: 18)
                        .fill(LinearGradient(
                            colors: [.white.opacity(inputFocused ? 0.14 : 0.08), .white.opacity(0.02)],
                            startPoint: .topLeading, endPoint: .bottomTrailing
                        ))
                }
            }
            .overlay {
                RoundedRectangle(cornerRadius: 18)
                    .strokeBorder(
                        LinearGradient(
                            colors: [
                                inputFocused ? Color.accentColor.opacity(0.6) : Color.white.opacity(0.30),
                                Color.white.opacity(0.06)
                            ],
                            startPoint: .topLeading, endPoint: .bottomTrailing
                        ),
                        lineWidth: inputFocused ? 1.0 : 0.75
                    )
            }
            .shadow(color: .black.opacity(0.06), radius: 4, y: 2)
            .animation(.easeInOut(duration: 0.15), value: inputFocused)

            // Paste button
            Button {
                if let string = NSPasteboard.general.string(forType: .string) {
                    downloadManager.startDownload(url: string)
                }
            } label: {
                Image(systemName: "doc.on.clipboard")
                    .font(.system(size: 14))
                    .foregroundStyle(.secondary)
                    .frame(width: 32, height: 32)
                    .background(.ultraThinMaterial)
                    .overlay {
                        RoundedRectangle(cornerRadius: 8)
                            .strokeBorder(Color.white.opacity(0.20), lineWidth: 0.75)
                    }
                    .clipShape(RoundedRectangle(cornerRadius: 8))
            }
            .buttonStyle(.plain)
            .help("Download from clipboard")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
    }

    // MARK: - Downloads Section

    private var downloadsSection: some View {
        VStack(alignment: .leading, spacing: 0) {
            if activeDownloads.isEmpty {
                HStack(spacing: 10) {
                    ZStack {
                        Circle().fill(.ultraThinMaterial)
                            .overlay { Circle().fill(Color.white.opacity(0.06)) }
                            .frame(width: 28, height: 28)
                        Image(systemName: "tray")
                            .font(.system(size: 12)).foregroundStyle(.tertiary)
                    }
                    Text("No active downloads")
                        .font(.system(size: 12)).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 14)
                .padding(.vertical, 13)
            } else {
                VStack(spacing: 6) {
                    ForEach(activeDownloads) { dl in
                        MenuBarDownloadRow(download: dl)
                    }
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 10)
            }
        }
    }

    // MARK: - Footer

    private var footerSection: some View {
        HStack(spacing: 10) {
            // Stats pills
            let active    = downloadManager.downloads.filter { $0.status == .downloading }.count
            let completed = downloadManager.downloads.filter { $0.status == .completed }.count

            HStack(spacing: 6) {
                statPill(
                    icon: "arrow.down",
                    label: "\(active) active",
                    color: active > 0 ? Color.accentColor : Color.secondary
                )
                statPill(
                    icon: "checkmark",
                    label: "\(completed) done",
                    color: completed > 0 ? Color.green : Color.secondary
                )
            }

            Spacer()

            // Action buttons
            HStack(spacing: 6) {
                Button("Open") {
                    NSApp.activate(ignoringOtherApps: true)
                    if let w = NSApp.windows.first(where: { $0.title.contains("KGet") || $0.isMainWindow }) {
                        w.makeKeyAndOrderFront(nil)
                    }
                }
                .buttonStyle(GlassButtonStyle())

                Button("Quit") { NSApplication.shared.terminate(nil) }
                    .buttonStyle(GlassButtonStyle(isDestructive: true))
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
    }

    private func statPill(icon: String, label: String, color: Color) -> some View {
        HStack(spacing: 4) {
            Image(systemName: icon).font(.system(size: 9, weight: .semibold))
            Text(label).font(.system(size: 10, weight: .medium))
        }
        .foregroundStyle(color)
        .padding(.horizontal, 7)
        .padding(.vertical, 4)
        .background(color.opacity(0.10))
        .clipShape(Capsule())
        .overlay { Capsule().strokeBorder(color.opacity(0.20), lineWidth: 0.5) }
    }
}

// MARK: - Glass Button Style

struct GlassButtonStyle: ButtonStyle {
    var isDestructive: Bool = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(isDestructive ? Color.red : Color.primary)
            .padding(.horizontal, 12)
            .padding(.vertical, 5)
            .background {
                ZStack {
                    RoundedRectangle(cornerRadius: 7)
                        .fill(.ultraThinMaterial)
                    RoundedRectangle(cornerRadius: 7)
                        .fill(Color.white.opacity(configuration.isPressed ? 0.05 : 0.10))
                }
            }
            .overlay {
                RoundedRectangle(cornerRadius: 7)
                    .strokeBorder(
                        LinearGradient(
                            colors: [Color.white.opacity(0.35), Color.white.opacity(0.08)],
                            startPoint: .topLeading, endPoint: .bottomTrailing
                        ),
                        lineWidth: 0.75
                    )
            }
            .scaleEffect(configuration.isPressed ? 0.97 : 1.0)
            .animation(.easeInOut(duration: 0.1), value: configuration.isPressed)
    }
}

// MARK: - Menu Bar Download Row

struct MenuBarDownloadRow: View {
    let download: Download
    @State private var isHovered = false

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 8) {
                // Animated status dot
                ZStack {
                    Circle().fill(Color.accentColor.opacity(0.22)).frame(width: 10, height: 10)
                    Circle().fill(Color.accentColor).frame(width: 6, height: 6)
                }
                .shadow(color: .accentColor.opacity(0.55), radius: 3)

                Text(download.filename)
                    .font(.system(size: 12, weight: .medium))
                    .lineLimit(1)

                Spacer()

                Text("\(Int(download.progress))%")
                    .font(.system(size: 11, weight: .semibold, design: .monospaced))
                    .foregroundStyle(Color.accentColor)
            }

            LiquidProgressBar(progress: download.progress / 100.0, color: .accentColor)
                .frame(height: 4)

            if !download.speed.isEmpty || !download.eta.isEmpty {
                HStack(spacing: 10) {
                    if !download.speed.isEmpty {
                        HStack(spacing: 3) {
                            Image(systemName: "arrow.down").font(.system(size: 9))
                            Text(download.speed).monospacedDigit()
                        }.foregroundStyle(.secondary)
                    }
                    if !download.eta.isEmpty {
                        HStack(spacing: 3) {
                            Image(systemName: "clock").font(.system(size: 9))
                            Text(download.eta).monospacedDigit()
                        }.foregroundStyle(.secondary)
                    }
                }
                .font(.caption2)
            }
        }
        .padding(.horizontal, 11)
        .padding(.vertical, 9)
        .glassCard(cornerRadius: 9, isHovered: isHovered)
        .onHover { isHovered = $0 }
        .animation(.easeInOut(duration: 0.15), value: isHovered)
    }
}

#if DEBUG
#Preview {
    MenuBarView().environmentObject(DownloadManager.shared)
}
#endif
