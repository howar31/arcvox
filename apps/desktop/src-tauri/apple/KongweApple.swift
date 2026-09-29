// C entry points over the Speech framework's on-device dictation.
// Every entry point returns an Int32 status: 0 ok, 1 needs download,
// 2 locale unsupported, 3 requires macOS 26, 8 cancelled, 9 failed.
// Callers must not call these on the main thread: each call blocks the
// calling thread until the async work finishes.

import AVFoundation
import Foundation

#if canImport(Speech)
import Speech
#endif

private func cString(_ s: String) -> UnsafeMutablePointer<CChar>? { strdup(s) }

@_cdecl("kw_apple_free")
public func kw_apple_free(_ p: UnsafeMutablePointer<CChar>?) { free(p) }

private final class Box<T>: @unchecked Sendable { var value: T? }

/// Runs `op` on the cooperative pool and waits for it. Returns nil on the main
/// thread, where waiting could deadlock work the framework schedules there.
private func runBlocking<T>(_ op: @escaping @Sendable () async -> T) -> T? {
    if Thread.isMainThread { return nil }
    let sem = DispatchSemaphore(value: 0)
    let box = Box<T>()
    Task.detached {
        box.value = await op()
        sem.signal()
    }
    sem.wait()
    return box.value
}

@_cdecl("kw_apple_dictation_status")
public func kw_apple_dictation_status(_ locale: UnsafePointer<CChar>) -> Int32 {
    if #available(macOS 26, *) {
        let id = String(cString: locale)
        return runBlocking { () async -> Int32 in
            guard let loc = await DictationTranscriber.supportedLocale(equivalentTo: Locale(identifier: id)) else {
                return 2
            }
            let transcriber = DictationTranscriber(locale: loc, preset: .longDictation)
            switch await AssetInventory.status(forModules: [transcriber]) {
            case .installed: return 0
            case .supported, .downloading: return 1
            case .unsupported: return 2
            @unknown default: return 9
            }
        } ?? 9
    }
    return 3
}

@_cdecl("kw_apple_dictation_install")
public func kw_apple_dictation_install(
    _ locale: UnsafePointer<CChar>,
    _ progress: @convention(c) (Double, UnsafeMutableRawPointer?) -> Void,
    _ user: UnsafeMutableRawPointer?
) -> Int32 {
    if #available(macOS 26, *) {
        let id = String(cString: locale)
        nonisolated(unsafe) let user = user
        return runBlocking { () async -> Int32 in
            do {
                guard let loc = await DictationTranscriber.supportedLocale(equivalentTo: Locale(identifier: id)) else {
                    return 2
                }
                let transcriber = DictationTranscriber(locale: loc, preset: .longDictation)
                try await AssetInventory.reserve(locale: loc)
                if let req = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
                    // Progress is polled from a child task rather than observed, so
                    // no callback can outlive this call: the group awaits its children.
                    try await withThrowingTaskGroup(of: Void.self) { group in
                        group.addTask { try await req.downloadAndInstall() }
                        group.addTask {
                            var last = -1.0
                            while !Task.isCancelled {
                                let f = req.progress.fractionCompleted
                                if f != last {
                                    progress(f, user)
                                    last = f
                                }
                                try? await Task.sleep(nanoseconds: 100_000_000)
                            }
                        }
                        try await group.next()
                        group.cancelAll()
                    }
                }
                progress(1.0, user)
                return 0
            } catch {
                return 9
            }
        } ?? 9
    }
    return 3
}

private func decodeStrings(_ json: UnsafePointer<CChar>?) -> [String] {
    guard let json else { return [] }
    let data = Data(String(cString: json).utf8)
    return (try? JSONDecoder().decode([String].self, from: data)) ?? []
}

@_cdecl("kw_apple_dictation_transcribe")
public func kw_apple_dictation_transcribe(
    _ locale: UnsafePointer<CChar>,
    _ pcm: UnsafePointer<Float>,
    _ len: Int,
    _ sampleRate: Int32,
    _ contextualJson: UnsafePointer<CChar>?,
    _ cancel: UnsafePointer<UInt8>?,
    _ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>
) -> Int32 {
    out.pointee = nil
    if #available(macOS 26, *) {
        guard sampleRate > 0 else { return 9 }
        if len == 0 {
            out.pointee = cString("")
            return out.pointee == nil ? 9 : 0
        }
        let id = String(cString: locale)
        let samples = Array(UnsafeBufferPointer(start: pcm, count: len))
        let strings = decodeStrings(contextualJson)
        nonisolated(unsafe) let cancel = cancel
        let isCancelled: @Sendable () -> Bool = { (cancel?.pointee ?? 0) != 0 }
        let result = runBlocking { () async -> (Int32, String) in
            do {
                return try await transcribe(
                    id: id, samples: samples, sampleRate: Double(sampleRate),
                    strings: strings, isCancelled: isCancelled)
            } catch {
                return (isCancelled() ? 8 : 9, "")
            }
        }
        guard let (code, text) = result else { return 9 }
        if code == 0 { out.pointee = cString(text) }
        return code
    }
    return 3
}

@available(macOS 26, *)
private func transcribe(
    id: String, samples: [Float], sampleRate: Double, strings: [String],
    isCancelled: @escaping @Sendable () -> Bool
) async throws -> (Int32, String) {
    guard let loc = await DictationTranscriber.supportedLocale(equivalentTo: Locale(identifier: id)) else {
        return (2, "")
    }
    let transcriber = DictationTranscriber(locale: loc, preset: .longDictation)
    guard await AssetInventory.status(forModules: [transcriber]) == .installed else { return (1, "") }
    let analyzer = SpeechAnalyzer(modules: [transcriber])
    if !strings.isEmpty {
        let ctx = AnalysisContext()
        ctx.contextualStrings = [.general: Array(strings.prefix(100))]
        try await analyzer.setContext(ctx)
    }
    guard let fmt = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [transcriber]),
          let buffer = convert(samples, sampleRate: sampleRate, to: fmt)
    else { return (9, "") }

    let collector = Task { () throws -> String in
        var text = ""
        for try await result in transcriber.results where result.isFinal {
            text += String(result.text.characters)
        }
        return text
    }
    // The analyzer has no cancel hook of its own; poll the caller's flag.
    let watcher = Task {
        while !Task.isCancelled {
            if isCancelled() {
                await analyzer.cancelAndFinishNow()
                return
            }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
    }

    let outcome: Result<String, any Error>
    do {
        let (stream, continuation) = AsyncStream<AnalyzerInput>.makeStream()
        continuation.yield(AnalyzerInput(buffer: buffer))
        continuation.finish()
        try await analyzer.start(inputSequence: stream)
        try await analyzer.finalizeAndFinishThroughEndOfInput()
        outcome = .success(try await collector.value)
    } catch {
        outcome = .failure(error)
    }
    // Both tasks must have finished before returning: the watcher reads the
    // caller's flag, which the caller frees once this call returns.
    watcher.cancel()
    await watcher.value
    if case .failure = outcome {
        await analyzer.cancelAndFinishNow()
        collector.cancel()
        _ = try? await collector.value
    }
    let text = try outcome.get()
    if isCancelled() { return (8, "") }
    return (0, text)
}

private func convert(_ samples: [Float], sampleRate: Double, to fmt: AVAudioFormat) -> AVAudioPCMBuffer? {
    guard let inFmt = AVAudioFormat(
        commonFormat: .pcmFormatFloat32, sampleRate: sampleRate, channels: 1, interleaved: false),
        let inBuf = AVAudioPCMBuffer(pcmFormat: inFmt, frameCapacity: AVAudioFrameCount(samples.count)),
        let converter = AVAudioConverter(from: inFmt, to: fmt)
    else { return nil }
    inBuf.frameLength = AVAudioFrameCount(samples.count)
    samples.withUnsafeBufferPointer { src in
        inBuf.floatChannelData![0].update(from: src.baseAddress!, count: samples.count)
    }
    let capacity = AVAudioFrameCount(Double(samples.count) * fmt.sampleRate / sampleRate) + 1024
    guard let outBuf = AVAudioPCMBuffer(pcmFormat: fmt, frameCapacity: capacity) else { return nil }
    var err: NSError?
    var consumed = false
    let status = converter.convert(to: outBuf, error: &err) { _, inputStatus in
        if consumed {
            inputStatus.pointee = .endOfStream
            return nil
        }
        consumed = true
        inputStatus.pointee = .haveData
        return inBuf
    }
    if status == .error || err != nil { return nil }
    return outBuf
}
