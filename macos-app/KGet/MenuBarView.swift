//
//  MenuBarView.swift
//  KGet
//
//  Menu bar dropdown view
//

import SwiftUI

struct MenuBarView: View {
    @EnvironmentObject var downloadManager: DownloadManager
    @State private var quickURL = ""
    
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Quick download input
            HStack {
                TextField("Quick download URL...", text: $quickURL)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 200)
                    .onSubmit {
                        if !quickURL.isEmpty {
                            downloadManager.startDownload(url: quickURL)
                            quickURL = ""
                        }
                    }
                
                Button(action: {
                    if let string = NSPasteboard.general.string(forType: .string) {
                        downloadManager.startDownload(url: string)
                    }
                }) {
                    Image(systemName: "doc.on.clipboard")
                }
                .help("Download from clipboard")
            }
            
            Divider()
            
            // Active downloads
            if downloadManager.downloads.filter({ $0.status == .downloading }).isEmpty {
                Text("No active downloads")
                    .font(.caption)
                    .foregroundColor(.secondary)
            } else {
                ForEach(downloadManager.downloads.filter { $0.status == .downloading }.prefix(5)) { download in
                    MenuBarDownloadRow(download: download)
                }
            }
            
            Divider()
            
            // Stats
            HStack {
                Label("\(downloadManager.downloads.filter { $0.status == .downloading }.count) active", systemImage: "arrow.down")
                Spacer()
                Label("\(downloadManager.downloads.filter { $0.status == .completed }.count) completed", systemImage: "checkmark")
            }
            .font(.caption)
            .foregroundColor(.secondary)
            
            Divider()
            
            // Actions
            HStack {
                Button("Open KGet") {
                    NSApp.activate(ignoringOtherApps: true)
                    if let window = NSApp.windows.first(where: { $0.title.contains("KGet") || $0.isMainWindow }) {
                        window.makeKeyAndOrderFront(nil)
                    }
                }
                
                Spacer()
                
                Button("Quit") {
                    NSApplication.shared.terminate(nil)
                }
            }
        }
        .padding()
        .frame(width: 320)
    }
}

struct MenuBarDownloadRow: View {
    let download: Download
    
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(download.filename)
                .font(.caption)
                .lineLimit(1)
            
            HStack {
                ProgressView(value: download.progress, total: 100)
                    .progressViewStyle(.linear)
                
                Text("\(Int(download.progress))%")
                    .font(.caption2)
                    .monospacedDigit()
            }
        }
    }
}

#if DEBUG
#Preview {
    MenuBarView()
        .environmentObject(DownloadManager.shared)
}
#endif
