//
//  SettingsView.swift
//  KGet
//
//  App settings view with ISO verification options
//

import SwiftUI

struct SettingsView: View {
    @EnvironmentObject var downloadManager: DownloadManager
    @State private var downloadPath: String = ""
    @State private var showNotifications: Bool = true
    @State private var speedLimit: String = ""
    @State private var useAdvancedByDefault: Bool = false
    @State private var verifyISOIntegrity: Bool = true
    
    var body: some View {
        TabView {
            // General Settings
            Form {
                Section("Downloads") {
                    HStack {
                        TextField("Download Location", text: $downloadPath)
                            .disabled(true)
                        
                        Button("Choose...") {
                            selectDownloadFolder()
                        }
                    }
                    
                    Toggle("Show notifications on completion", isOn: $showNotifications)
                }
            }
            .padding()
            .tabItem {
                Label("General", systemImage: "gear")
            }
            
            // Downloads Settings  
            Form {
                Section("Download Options") {
                    Toggle("Use advanced mode by default", isOn: $useAdvancedByDefault)
                        .help("Downloads files in parallel chunks for faster speed")
                    
                    HStack {
                        Text("Speed limit (KB/s):")
                        TextField("0 = unlimited", text: $speedLimit)
                            .frame(width: 100)
                    }
                }
                
                Section("ISO Files") {
                    Toggle("Auto-verify ISO integrity", isOn: $verifyISOIntegrity)
                        .help("Automatically calculate SHA256 checksum after downloading ISO files")
                    
                    Text("When enabled, KGet will calculate the SHA256 hash after downloading .iso files to verify their integrity.")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
            }
            .padding()
            .tabItem {
                Label("Downloads", systemImage: "arrow.down.circle")
            }
            
            // About
            VStack(spacing: 20) {
                // Logo
                LogoView()
                    .frame(width: 80, height: 80)
                
                Text("KGet")
                    .font(.largeTitle)
                    .fontWeight(.bold)
                
                Text("Version \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "1.6.3")")
                    .foregroundColor(.secondary)
                
                Text("A modern, lightweight download manager\nbuilt with Rust and SwiftUI")
                    .multilineTextAlignment(.center)
                    .foregroundColor(.secondary)
                    .font(.callout)
                
                Divider()
                    .frame(width: 200)
                
                VStack(spacing: 8) {
                    Link(destination: URL(string: "https://github.com/davimf721/KGet")!) {
                        Label("GitHub Repository", systemImage: "link")
                    }
                    
                    Link(destination: URL(string: "https://github.com/davimf721/KGet/issues")!) {
                        Label("Report an Issue", systemImage: "exclamationmark.bubble")
                    }
                    
                    Link(destination: URL(string: "https://crates.io/crates/Kget")!) {
                        Label("crates.io", systemImage: "shippingbox")
                    }
                }
                
                Spacer()
                
                Text("© 2026 Davi Moreira Fuzatto")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }
            .padding()
            .tabItem {
                Label("About", systemImage: "info.circle")
            }
        }
        .frame(width: 480, height: 340)
        .onAppear {
            downloadPath = downloadManager.defaultDownloadPath.path
            verifyISOIntegrity = downloadManager.verifyISOIntegrity
            showNotifications = downloadManager.showNotifications
            useAdvancedByDefault = downloadManager.useAdvancedByDefault
            speedLimit = downloadManager.speedLimitKBPerSecond
        }
        .onChange(of: verifyISOIntegrity) { newValue in
            downloadManager.verifyISOIntegrity = newValue
            downloadManager.saveSettings()
        }
        .onChange(of: showNotifications) { newValue in
            downloadManager.showNotifications = newValue
            downloadManager.saveSettings()
        }
        .onChange(of: useAdvancedByDefault) { newValue in
            downloadManager.useAdvancedByDefault = newValue
            downloadManager.saveSettings()
        }
        .onChange(of: speedLimit) { newValue in
            downloadManager.speedLimitKBPerSecond = newValue
            downloadManager.saveSettings()
        }
    }
    
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
    
    var isISO: Bool {
        url.lowercased().hasSuffix(".iso")
    }
    
    var body: some View {
        VStack(spacing: 20) {
            // Header with logo
            HStack {
                LogoView()
                    .frame(width: 32, height: 32)
                
                Text("New Download")
                    .font(.headline)
                
                Spacer()
            }
            
            Form {
                Section {
                    TextField("URL:", text: $url)
                        .textFieldStyle(.roundedBorder)
                    
                    HStack {
                        TextField("Save to:", text: $outputPath)
                            .textFieldStyle(.roundedBorder)
                        
                        Button("Browse...") {
                            selectOutputPath()
                        }
                    }
                }
                
                Section {
                    Toggle("Advanced mode (parallel chunks)", isOn: $useAdvancedMode)
                        .help("Downloads file in multiple parallel chunks for faster speed")
                    
                    if isISO {
                        Toggle("Verify checksum after download", isOn: $verifyChecksum)
                            .help("Calculate SHA256 hash to verify file integrity")

                        TextField("Expected SHA256 (optional):", text: $expectedSHA256)
                            .textFieldStyle(.roundedBorder)
                            .font(.system(.body, design: .monospaced))
                        
                        HStack {
                            Image(systemName: "info.circle")
                                .foregroundColor(.blue)
                            Text("ISO file detected - integrity verification recommended")
                                .font(.caption)
                                .foregroundColor(.secondary)
                        }
                    }
                }
            }

            if let validationError = validationError ?? downloadManager.lastStartError {
                HStack {
                    Image(systemName: "exclamationmark.triangle.fill")
                    Text(validationError)
                }
                .font(.caption)
                .foregroundColor(.red)
            }
            
            HStack {
                Button("Cancel") {
                    dismiss()
                }
                .keyboardShortcut(.escape)
                
                Spacer()
                
                Button("Download") {
                    startDownload()
                }
                .keyboardShortcut(.return)
                .buttonStyle(.borderedProminent)
                .disabled(url.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding()
        .frame(width: 450)
        .onAppear {
            outputPath = downloadManager.defaultDownloadPath.path
            verifyChecksum = downloadManager.verifyISOIntegrity
            useAdvancedMode = downloadManager.useAdvancedByDefault
            
            // Auto-paste from clipboard if it's a URL
            if let clipboardString = NSPasteboard.general.string(forType: .string),
               clipboardString.hasPrefix("http") || clipboardString.hasPrefix("magnet:") || clipboardString.hasPrefix("ftp:") {
                url = clipboardString
            }
        }
    }
    
    private func selectOutputPath() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Select"
        
        if panel.runModal() == .OK, let selectedURL = panel.url {
            outputPath = selectedURL.path
        }
    }
    
    private func startDownload() {
        let didStart = downloadManager.startDownload(
            url: url,
            outputPath: outputPath,
            advanced: useAdvancedMode,
            verifyISO: verifyChecksum,
            expectedSHA256: expectedSHA256
        )
        if didStart {
            dismiss()
        } else {
            validationError = downloadManager.lastStartError
        }
    }
}

#if DEBUG
#Preview {
    SettingsView()
        .environmentObject(DownloadManager.shared)
}
#endif
