// swift-tools-version: 6.2

import Foundation
import PackageDescription

let onTheHost = ProcessInfo.processInfo.environment["RESONATE_HOST_TESTS"] != nil

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let native = root.appendingPathComponent("Generated/lib").path
let host = root.appendingPathComponent(".cargo-target/debug").path
let intentProtocols = root.appendingPathComponent("Resonate/AppIntentsProtocols.json").path

let app: [Target] = [
    .target(
        name: "Resonate",
        dependencies: ["ResonateNative", "ResonateLogic"],
        path: "Sources/Resonate",
        swiftSettings: [
            .unsafeFlags([
                "-emit-const-values",
                "-Xfrontend", "-const-gather-protocols-file",
                "-Xfrontend", intentProtocols,
            ])
        ]
    )
]

let package = Package(
    name: "Resonate",
    platforms: [.iOS(.v26)],
    products: onTheHost ? [] : [.library(name: "Resonate", targets: ["Resonate"])],
    targets: [
        .systemLibrary(name: "ResonateNativeFFI", path: "Generated/ResonateNativeFFI"),
        .target(
            name: "ResonateNative",
            dependencies: ["ResonateNativeFFI"],
            path: "Generated/ResonateNative",
            linkerSettings: [
                .unsafeFlags(["-L", native], .when(platforms: [.iOS])),
                .unsafeFlags(["-L", host, "-Xlinker", "-rpath", "-Xlinker", host], .when(platforms: [.linux])),
                .linkedFramework("AudioToolbox", .when(platforms: [.iOS])),
                .linkedFramework("CoreAudio", .when(platforms: [.iOS])),
                .linkedFramework("Security", .when(platforms: [.iOS])),
                .linkedFramework("SystemConfiguration", .when(platforms: [.iOS])),
            ]
        ),
        .target(
            name: "ResonateLogic",
            dependencies: ["ResonateNative"],
            path: "Sources/ResonateLogic"
        ),
        .testTarget(
            name: "ResonateLogicTests",
            dependencies: ["ResonateLogic"],
            path: "Tests/ResonateLogicTests"
        ),
    ] + (onTheHost ? [] : app)
)
