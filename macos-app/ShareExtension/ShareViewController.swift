//
//  ShareViewController.swift
//  KGet Share Extension
//
//  Handles "Share > KGet" from Safari, Finder, and any macOS app.
//  Encodes the shared URL into a kget:// custom-scheme URL and
//  opens the main KGet app via NSWorkspace.
//

import Cocoa
import UniformTypeIdentifiers

class ShareViewController: NSViewController {

    override var nibName: NSNib.Name? { nil }

    override func loadView() {
        self.view = NSView(frame: NSRect(x: 0, y: 0, width: 0, height: 0))
        processSharedItems()
    }

    // MARK: - Item Processing

    private func processSharedItems() {
        guard
            let extensionItem = extensionContext?.inputItems.first as? NSExtensionItem,
            let attachments = extensionItem.attachments,
            !attachments.isEmpty
        else {
            complete()
            return
        }

        // Try URL first, then fall back to plain text
        let attachment = attachments[0]
        if attachment.hasItemConformingToTypeIdentifier(UTType.url.identifier) {
            attachment.loadItem(forTypeIdentifier: UTType.url.identifier, options: nil) { [weak self] item, _ in
                if let url = item as? URL {
                    self?.openInKGet(url.absoluteString)
                } else {
                    self?.complete()
                }
            }
        } else if attachment.hasItemConformingToTypeIdentifier(UTType.plainText.identifier) {
            attachment.loadItem(forTypeIdentifier: UTType.plainText.identifier, options: nil) { [weak self] item, _ in
                if let text = item as? String, Self.looksLikeDownloadURL(text) {
                    self?.openInKGet(text)
                } else {
                    self?.complete()
                }
            }
        } else {
            complete()
        }
    }

    // MARK: - Open KGet

    /// Encodes `urlString` as a query param in a `kget://download?url=…` URL
    /// and asks NSWorkspace to open it, which activates the main app.
    private func openInKGet(_ urlString: String) {
        guard
            let encoded = urlString.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed),
            let kgetURL = URL(string: "kget://download?url=\(encoded)")
        else {
            complete()
            return
        }

        DispatchQueue.main.async {
            NSWorkspace.shared.open(kgetURL)
            self.complete()
        }
    }

    private func complete() {
        extensionContext?.completeRequest(returningItems: [], completionHandler: nil)
    }

    // MARK: - Helpers

    /// Returns true for HTTP(S), FTP, SFTP, WebDAV, and magnet URLs.
    static func looksLikeDownloadURL(_ s: String) -> Bool {
        let lower = s.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return lower.hasPrefix("http://")
            || lower.hasPrefix("https://")
            || lower.hasPrefix("ftp://")
            || lower.hasPrefix("sftp://")
            || lower.hasPrefix("webdav://")
            || lower.hasPrefix("webdavs://")
            || lower.hasPrefix("magnet:")
    }

    // MARK: - IBActions (called by the system if a nib were present)

    @IBAction func send(_ sender: AnyObject?) {
        extensionContext?.completeRequest(returningItems: [], completionHandler: nil)
    }

    @IBAction func cancel(_ sender: AnyObject?) {
        let err = NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError)
        extensionContext?.cancelRequest(withError: err)
    }
}
