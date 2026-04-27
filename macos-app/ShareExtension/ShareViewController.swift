//
//  ShareViewController.swift
//  KGet Share Extension
//
//  Allows sharing URLs to KGet from Safari and other apps
//

import Cocoa
import UniformTypeIdentifiers

class ShareViewController: NSViewController {
    
    override var nibName: NSNib.Name? {
        return NSNib.Name("ShareViewController")
    }

    override func loadView() {
        self.view = NSView()
        processSharedItems()
    }
    
    private func processSharedItems() {
        guard let extensionItem = extensionContext?.inputItems.first as? NSExtensionItem,
              let attachments = extensionItem.attachments else {
            self.extensionContext?.completeRequest(returningItems: [], completionHandler: nil)
            return
        }
        
        for attachment in attachments {
            // Handle URLs
            if attachment.hasItemConformingToTypeIdentifier(UTType.url.identifier) {
                attachment.loadItem(forTypeIdentifier: UTType.url.identifier, options: nil) { [weak self] item, error in
                    if let url = item as? URL {
                        self?.downloadWithKGet(url: url.absoluteString)
                    }
                }
            }
            // Handle plain text (might be a URL)
            else if attachment.hasItemConformingToTypeIdentifier(UTType.plainText.identifier) {
                attachment.loadItem(forTypeIdentifier: UTType.plainText.identifier, options: nil) { [weak self] item, error in
                    if let text = item as? String, 
                       text.hasPrefix("http") || text.hasPrefix("magnet:") || text.hasPrefix("ftp:") {
                        self?.downloadWithKGet(url: text)
                    }
                }
            }
        }
    }
    
    private func downloadWithKGet(url: String) {
        // Open KGet with the URL using custom URL scheme
        let kgetURL = URL(string: "kget://\(url.replacingOccurrences(of: "https://", with: "").replacingOccurrences(of: "http://", with: ""))")!
        
        DispatchQueue.main.async {
            NSWorkspace.shared.open(kgetURL)
            self.extensionContext?.completeRequest(returningItems: [], completionHandler: nil)
        }
    }
    
    @IBAction func send(_ sender: AnyObject?) {
        self.extensionContext?.completeRequest(returningItems: [], completionHandler: nil)
    }

    @IBAction func cancel(_ sender: AnyObject?) {
        let cancelError = NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError, userInfo: nil)
        self.extensionContext?.cancelRequest(withError: cancelError)
    }
}
