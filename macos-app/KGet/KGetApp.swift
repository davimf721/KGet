//
//  KGetApp.swift
//  KGet - Modern Download Manager
//
//  Native macOS wrapper for the Rust KGet binary
//

import SwiftUI
import UserNotifications

@main
struct KGetApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    @StateObject private var downloadManager = DownloadManager.shared
    
    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(downloadManager)
                .onOpenURL { url in
                    handleURL(url)
                }
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Download...") {
                    downloadManager.showNewDownloadSheet = true
                }
                .keyboardShortcut("n", modifiers: .command)
            }
        }
        
        // Menu Bar Extra (always visible in menu bar)
        MenuBarExtra("KGet", systemImage: "arrow.down.circle.fill") {
            MenuBarView()
                .environmentObject(downloadManager)
        }
        .menuBarExtraStyle(.window)
        
        Settings {
            SettingsView()
                .environmentObject(downloadManager)
        }
    }
    
    private func handleURL(_ url: URL) {
        // Handle kget:// URLs
        if url.scheme == "kget" {
            // Convert kget://example.com/file.zip to https://example.com/file.zip
            var components = URLComponents(url: url, resolvingAgainstBaseURL: false)
            components?.scheme = "https"
            if let downloadURL = components?.url {
                downloadManager.startDownload(url: downloadURL.absoluteString)
            }
        } else if url.scheme == "magnet" {
            // Use KGet's native torrent client for magnet links
            downloadManager.startDownload(url: url.absoluteString)
        } else if url.isFileURL && url.pathExtension == "torrent" {
            // .torrent files not supported yet
            appDelegate.showTorrentFileWarning()
        } else if url.isFileURL {
            // Other files - try to download
            downloadManager.startDownload(url: url.path)
        }
    }
}

// MARK: - App Delegate
class AppDelegate: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Request notification permissions
        UNUserNotificationCenter.current().delegate = self
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { granted, error in
            if granted {
                print("Notification permission granted")
            }
        }
        
        // Register for Services menu
        NSApp.servicesProvider = ServiceProvider.shared
        NSUpdateDynamicServices()
    }
    
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return false // Keep running in menu bar
    }
    
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls {
            if url.pathExtension == "torrent" {
                // .torrent files are not supported yet (only magnet links)
                showTorrentFileWarning()
            } else if url.scheme == "magnet" {
                // Use KGet's native torrent client for magnet links
                DownloadManager.shared.startDownload(url: url.absoluteString)
            }
        }
    }
    
    /// Shows a warning that .torrent files are not supported (only magnet links)
    func showTorrentFileWarning() {
        DispatchQueue.main.async {
            let alert = NSAlert()
            alert.messageText = "Torrent Files Not Supported"
            alert.informativeText = "KGet currently supports magnet links only. Please use the magnet link instead of the .torrent file, or use a dedicated BitTorrent client."
            alert.alertStyle = .informational
            alert.addButton(withTitle: "OK")
            alert.runModal()
        }
    }
    
    // Handle notifications when app is in foreground
    func userNotificationCenter(_ center: UNUserNotificationCenter, 
                                willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .sound])
    }
}

// MARK: - Services Provider (for macOS Services menu)
class ServiceProvider: NSObject {
    static let shared = ServiceProvider()
    
    @objc func downloadURL(_ pboard: NSPasteboard, userData: String, error: AutoreleasingUnsafeMutablePointer<NSString?>) {
        guard let items = pboard.pasteboardItems else { return }
        
        for item in items {
            if let urlString = item.string(forType: .URL) ?? item.string(forType: .string) {
                DownloadManager.shared.startDownload(url: urlString)
            }
        }
    }
}
