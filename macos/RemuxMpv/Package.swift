// swift-tools-version:5.9
import PackageDescription

// The player for the desktop app on a Mac: mpv (MPVKit, the build with a macOS library) with the
// C interface the Rust side calls, the same as on iOS (`ios/RemuxMpv`, which has the build of
// Streamyfin with another video output, that has no macOS library). Built into a dynamic
// framework by `cargo make macos-framework`.
let package = Package(
    name: "RemuxMpv",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "RemuxMpv", type: .dynamic, targets: ["RemuxMpv"]),
    ],
    dependencies: [
        .package(url: "https://github.com/mpvkit/MPVKit.git", exact: "0.41.0"),
    ],
    targets: [
        .target(name: "RemuxMpv", dependencies: [.product(name: "MPVKit-GPL", package: "MPVKit")]),
    ]
)
