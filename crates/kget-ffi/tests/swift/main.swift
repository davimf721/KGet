// Smoke test for the generated Swift bindings (scripts/swift-smoke-test.sh).
// Loads libkget_ffi, creates an engine and exercises the basic calls.

import Foundation

final class Collector: KgetEventListener, @unchecked Sendable {
    func onEvent(event: JobEventRecord) {}
}

func check(_ ok: Bool, _ what: String) {
    if !ok {
        FileHandle.standardError.write("FAIL: \(what)\n".data(using: .utf8)!)
        exit(1)
    }
}

let configDir = CommandLine.arguments.count > 1
    ? CommandLine.arguments[1]
    : NSTemporaryDirectory() + "kget-ffi-smoke"

let engine = try KgetEngine(configDir: configDir)
engine.setListener(listener: Collector())
check(engine.list().isEmpty, "a new engine has no jobs")
check(engine.get(id: 1) == nil, "unknown job")
check(!kgetVersion().isEmpty, "version")

engine.setMaxConcurrent(max: 2)
check(engine.maxConcurrent() == 2, "max concurrent")

do {
    try engine.cancel(id: 42)
    check(false, "cancel of an unknown job must throw")
} catch let error as EngineError {
    check(error == .NotFound(id: 42), "NotFound error, got \(error)")
}

do {
    _ = try engine.submit(request: DownloadRequest(url: ""))
    check(false, "an empty URL must throw")
} catch EngineError.InvalidRequest {
}

let history = try engine.history(filter: HistoryFilter())
check(history.isEmpty, "fresh history is empty")
engine.clearListener()

print("Swift smoke test passed (kget \(kgetVersion()), config \(configDir))")
