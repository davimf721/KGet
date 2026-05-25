// swift-tools-version: 5.9
// KGet macOS App Package

import PackageDescription

let package = Package(
    name: "KGet",
    platforms: [
        .macOS(.v13)
    ],
    targets: [
        .executableTarget(
            name: "KGet",
            path: "KGet",
            exclude: [
                "Info.plist",
                "KGet.entitlements"
            ],
            sources: [
                "KGetApp.swift",
                "DownloadManager.swift",
                "ContentView.swift",
                "MenuBarView.swift",
                "SettingsView.swift"
            ]
        )
    ]
)
