import Foundation
import AppKit
import IOKit.pwr_mgt
import Libmpv

/// A framework needs its own copy of the layer subclass of the MPVKit demo: MoltenVK sets the
/// drawable size to 1x1 to finish a presentation, which flickers.
final class MetalLayer: CAMetalLayer {
    override var drawableSize: CGSize {
        get { super.drawableSize }
        set {
            if Int(newValue.width) > 1 && Int(newValue.height) > 1 {
                super.drawableSize = newValue
            }
        }
    }

    /// The screen's EDR mode (HDR, `target-colorspace-hint`) only turns on when this is set on the
    /// main thread; mpv sets it from its own (MPVKit's demo).
    override var wantsExtendedDynamicRangeContent: Bool {
        get { super.wantsExtendedDynamicRangeContent }
        set {
            if Thread.isMainThread {
                super.wantsExtendedDynamicRangeContent = newValue
            } else {
                DispatchQueue.main.sync { super.wantsExtendedDynamicRangeContent = newValue }
            }
        }
    }
}

/// Keeps the layer as big as the view it is in.
final class VideoView: NSView {
    let metal = MetalLayer()

    override init(frame: NSRect) {
        super.init(frame: frame)
        // A black layer of the view's own, and in it the Metal layer mpv draws on, always the size of
        // the screen (see `fitLayer`).
        wantsLayer = true
        layer = CALayer()
        layer?.backgroundColor = NSColor.black.cgColor
        metal.contentsScale = NSScreen.main?.backingScaleFactor ?? 2
        metal.framebufferOnly = true
        metal.backgroundColor = NSColor.black.cgColor
        layer?.addSublayer(metal)
        fitLayer()
    }

    required init?(coder: NSCoder) { fatalError() }

    /// The app sets the view's frame itself (the player as large as the window, full screen), which
    /// asks for no layout: the layer follows it here too.
    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        fitLayer()
    }

    override func layout() {
        super.layout()
        fitLayer()
    }

    /// mpv takes the size of what it draws on once, as it starts, and keeps it (moltenvk): so the
    /// Metal layer is always as large as the screen, and it is scaled (at its proportions) to
    /// fit the view. Full screen that is the view's exact size; in a window it is scaled down.
    func fitLayer() {
        let screen = window?.screen ?? NSScreen.main
        let scale = screen?.backingScaleFactor ?? 2
        let size = screen?.frame.size ?? CGSize(width: 1920, height: 1080)
        let fit = min(bounds.width / size.width, bounds.height / size.height)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        metal.contentsScale = scale
        metal.bounds = CGRect(origin: .zero, size: size)
        metal.drawableSize = CGSize(width: size.width * scale, height: size.height * scale)
        metal.position = CGPoint(x: bounds.midX, y: bounds.midY)
        metal.setAffineTransform(CGAffineTransform(scaleX: fit > 0 ? fit : 1, y: fit > 0 ? fit : 1))
        CATransaction.commit()
    }
}

/// mpv in the desktop app on a Mac: draws on a Metal layer in a view of its own, which the app puts
/// where the player is (the native component `remux.mpv`, `remux_mpv_view`). Rust calls the `remux_mpv_*` functions below; they are those
/// of the iOS and Android players. mpv has the tracks of the file itself: the audio and the
/// subtitles (also the ones that are pictures, such as PGS) are chosen here, and the server need
/// not transcode.
private final class Player {
    static let shared = Player()

    var mpv: OpaquePointer?
    var video: VideoView?
    var loaded = false
    var failed: String?
    /// The file is open and its tracks are known: before that no track can be picked.
    var fileLoaded = false
    /// What the page asked for, as stream indices of the server's version of the file, applied
    /// once the tracks are there. -1 is none (for audio: not asked).
    var wantedSubtitle: Int64 = -1
    var wantedAudio: Int64 = -1
    var pendingSubtitles: [(index: String, label: String, language: String, url: String)] = []
    private var ticker: Timer?

    private func onMain<T>(_ work: () -> T) -> T {
        Thread.isMainThread ? work() : DispatchQueue.main.sync(execute: work)
    }

    /// The view the video is drawn in: one for the app, as mpv draws on its layer from the start
    /// (`wid`) on. The app puts it where the player is, each time a player is made.
    func view() -> VideoView {
        onMain {
            if let video { return video }
            let made = VideoView(frame: NSRect(x: 0, y: 0, width: 640, height: 360))
            video = made
            return made
        }
    }

    /// Starts mpv drawing on the view. Once.
    func start() -> Bool {
        onMain {
            if mpv != nil { return true }
            let view = self.view()

            guard let handle = mpv_create() else { return false }
            var layer: AnyObject = view.metal
            mpv_set_option(handle, "wid", MPV_FORMAT_INT64, &layer)
            let options: [(String, String)] = [
                ("vo", "gpu-next"),
                ("gpu-api", "vulkan"),
                ("gpu-context", "moltenvk"),
                ("hwdec", "videotoolbox"),
                // HDR and Dolby Vision mapped to what the screen shows (the app tells the server
                // the Mac plays them, so they come as they are).
                ("tone-mapping", "auto"),
                ("target-colorspace-hint", "yes"),
                ("hwdec-codecs", "all"),
                ("hwdec-software-fallback", "yes"),
                ("video-rotate", "no"),
                ("keep-open", "yes"),
                // The page has its own controls and subtitle menu.
                ("osc", "no"),
                ("input-default-bindings", "no"),
                ("sub-auto", "no"),
                ("sub-scale-with-window", "no"),
                ("sub-use-margins", "no"),
            ]
            for (name, value) in options {
                mpv_set_option_string(handle, name, value)
            }
            guard mpv_initialize(handle) >= 0 else { return false }
            mpv = handle

            // mpv says when the file is open: its tracks can be picked then, and not before.
            ticker = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
                self?.drainEvents()
            }
            return true
        }
    }

    private func drainEvents() {
        guard let mpv else { return }
        while true {
            guard let event = mpv_wait_event(mpv, 0), event.pointee.event_id != MPV_EVENT_NONE else { return }
            switch event.pointee.event_id {
            case MPV_EVENT_FILE_LOADED:
                fileLoaded = true
                for sub in pendingSubtitles {
                    // "cached": added, not picked; the page picks.
                    command(["sub-add", sub.url, "cached", "idx:\(sub.index) \(sub.label)", sub.language])
                }
                pendingSubtitles = []
                applyTracks()
            case MPV_EVENT_END_FILE:
                if let data = event.pointee.data?.assumingMemoryBound(to: mpv_event_end_file.self).pointee,
                   data.reason == MPV_END_FILE_REASON_ERROR {
                    failed = String(cString: mpv_error_string(data.error))
                }
            default:
                break
            }
        }
    }

    /// Picks the audio and the subtitle the page asked for, from the tracks mpv lists. The tracks
    /// of the file are found by the index they have in it, which is the stream index the server
    /// uses; the ones added from an address by the title that was given them.
    func applyTracks() {
        guard mpv != nil, fileLoaded else { return }
        let count = Int(double("track-list/count"))
        if wantedAudio >= 0 {
            for i in 0..<count where string("track-list/\(i)/type") == "audio" {
                if Int64(double("track-list/\(i)/ff-index")) == wantedAudio, let id = string("track-list/\(i)/id") {
                    setString("aid", id)
                    break
                }
            }
        }
        if wantedSubtitle < 0 {
            setString("sid", "no")
            return
        }
        for i in 0..<count where string("track-list/\(i)/type") == "sub" {
            let external = flag("track-list/\(i)/external")
            let title = string("track-list/\(i)/title") ?? ""
            let matches = external
                ? title.hasPrefix("idx:\(wantedSubtitle) ")
                : Int64(double("track-list/\(i)/ff-index")) == wantedSubtitle
            if matches, let id = string("track-list/\(i)/id") {
                setString("sid", id)
                return
            }
        }
    }

    func command(_ args: [String]) {
        guard let mpv else { return }
        var cStrings: [UnsafeMutablePointer<CChar>?] = args.map { strdup($0) }
        cStrings.append(nil)
        defer { cStrings.forEach { free($0) } }
        var pointers = cStrings.map { UnsafePointer<CChar>($0) }
        mpv_command(mpv, &pointers)
    }

    func double(_ name: String) -> Double {
        guard let mpv else { return 0 }
        var value = Double()
        return mpv_get_property(mpv, name, MPV_FORMAT_DOUBLE, &value) >= 0 ? value : 0
    }

    func flag(_ name: String) -> Bool {
        guard let mpv else { return false }
        var value: Int32 = 0
        return mpv_get_property(mpv, name, MPV_FORMAT_FLAG, &value) >= 0 && value > 0
    }

    func string(_ name: String) -> String? {
        guard let mpv, let raw = mpv_get_property_string(mpv, name) else { return nil }
        defer { mpv_free(raw) }
        return String(cString: raw)
    }

    func setFlag(_ name: String, _ value: Bool) {
        guard let mpv else { return }
        var data: Int32 = value ? 1 : 0
        mpv_set_property(mpv, name, MPV_FORMAT_FLAG, &data)
    }

    func setString(_ name: String, _ value: String) {
        guard let mpv else { return }
        mpv_set_property_string(mpv, name, value)
    }

    func setDouble(_ name: String, _ value: Double) {
        guard let mpv else { return }
        var data = value
        mpv_set_property(mpv, name, MPV_FORMAT_DOUBLE, &data)
    }
}

private func text(_ pointer: UnsafePointer<CChar>?) -> String {
    pointer.map { String(cString: $0) } ?? ""
}

/// `subtitles` is one "index<TAB>label<TAB>language<TAB>url" per line: the ones to add by address.
@_cdecl("remux_mpv_load")
public func remuxMpvLoad(
    _ url: UnsafePointer<CChar>?, _ startMs: Double, _ volume: Double, _ muted: Bool,
    _ rate: Double, _ subtitles: UnsafePointer<CChar>?
) {
    let player = Player.shared
    guard player.start() else {
        player.failed = "The player could not start"
        return
    }
    player.failed = nil
    player.loaded = false
    player.fileLoaded = false
    player.wantedSubtitle = -1
    player.wantedAudio = -1
    player.pendingSubtitles = text(subtitles).split(separator: "\n").compactMap { line in
        let parts = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
        return parts.count == 4 ? (index: parts[0], label: parts[1], language: parts[2], url: parts[3]) : nil
    }
    player.setString("volume", String(Int(volume * 100)))
    player.setFlag("mute", muted)
    player.setDouble("speed", rate)
    // Without a start there is no seek: for a live stream that is one to a time its playlist may
    // not have yet, which fails and leaves the decoder with pictures it cannot start from. The
    // hardware decoder then gives up, and the player decodes in software from there on.
    var arguments = ["loadfile", text(url), "replace", "-1"]
    if startMs > 0 { arguments.append("start=\(startMs / 1000)") }
    player.command(arguments)
    player.loaded = true
}

/// The view the video is drawn in (an `NSView`, retained once more for the caller), for the app to
/// put where the player is.
@_cdecl("remux_mpv_view")
public func remuxMpvView() -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(Player.shared.view()).toOpaque()
}

@_cdecl("remux_mpv_close")
public func remuxMpvClose() {
    let player = Player.shared
    player.command(["stop"])
    player.loaded = false
    player.fileLoaded = false
}

@_cdecl("remux_mpv_pause")
public func remuxMpvPause(_ paused: Bool) { Player.shared.setFlag("pause", paused) }

@_cdecl("remux_mpv_seek")
public func remuxMpvSeek(_ ms: Double) {
    Player.shared.command(["seek", String(ms / 1000), "absolute"])
}

@_cdecl("remux_mpv_volume")
public func remuxMpvVolume(_ volume: Double, _ muted: Bool) {
    Player.shared.setString("volume", String(Int(volume * 100)))
    Player.shared.setFlag("mute", muted)
}

/// The app's window into full screen, or out of it (the player's full screen button and `f`).
@_cdecl("remux_mpv_fullscreen")
public func remuxMpvFullscreen() {
    DispatchQueue.main.async {
        (Player.shared.video?.window ?? NSApp.keyWindow ?? NSApp.mainWindow)?.toggleFullScreen(nil)
    }
}

@_cdecl("remux_mpv_rate")
public func remuxMpvRate(_ rate: Double) { Player.shared.setDouble("speed", rate) }

/// The subtitle with this stream index of the server's version of the file; a negative one hides
/// them.
@_cdecl("remux_mpv_subtitle")
public func remuxMpvSubtitle(_ index: Int64) {
    let player = Player.shared
    player.wantedSubtitle = index
    player.applyTracks()
}

/// The audio track with this stream index; a negative one leaves the choice to mpv.
@_cdecl("remux_mpv_audio")
public func remuxMpvAudio(_ index: Int64) {
    let player = Player.shared
    player.wantedAudio = index
    player.applyTracks()
}

private var sleepAssertion: IOPMAssertionID = 0

@_cdecl("remux_mpv_keep_awake")
public func remuxMpvKeepAwake(_ on: Bool) {
    DispatchQueue.main.async {
        if on, sleepAssertion == 0 {
            IOPMAssertionCreateWithName(
                kIOPMAssertionTypePreventUserIdleDisplaySleep as CFString, IOPMAssertionLevel(kIOPMAssertionLevelOn),
                "Remux is playing" as CFString, &sleepAssertion)
        } else if !on, sleepAssertion != 0 {
            IOPMAssertionRelease(sleepAssertion)
            sleepAssertion = 0
        }
    }
}

/// Writes `position,duration,paused,ended,volume,muted,rate,buffered end,error` (ms and 0/1)
/// into `buffer`, and returns how many bytes that is.
@_cdecl("remux_mpv_snapshot")
public func remuxMpvSnapshot(_ buffer: UnsafeMutablePointer<CChar>?, _ capacity: Int32) -> Int32 {
    let player = Player.shared
    var line: String
    if player.mpv == nil || !player.loaded {
        line = "0,0,1,0,1,0,1,0,\(player.failed ?? "")"
    } else {
        let position = player.double("time-pos") * 1000
        let duration = player.double("duration") * 1000
        let cache = player.double("demuxer-cache-time")
        let buffered = cache > 0 ? position + (cache - player.double("time-pos")) * 1000 : position
        let failed = player.failed ?? (player.flag("idle-active") && !player.flag("eof-reached") && duration == 0
            ? "This file could not be played" : "")
        line = [
            String(position), String(duration), player.flag("pause") ? "1" : "0",
            player.flag("eof-reached") ? "1" : "0", String(player.double("volume") / 100),
            player.flag("mute") ? "1" : "0", String(player.double("speed")), String(buffered),
            failed.replacingOccurrences(of: ",", with: " "),
        ].joined(separator: ",")
    }
    guard let buffer else { return 0 }
    let bytes = Array(line.utf8.prefix(Int(capacity) - 1))
    for (i, b) in bytes.enumerated() { buffer[i] = CChar(bitPattern: b) }
    buffer[bytes.count] = 0
    return Int32(bytes.count)
}
