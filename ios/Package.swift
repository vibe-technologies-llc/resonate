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
        dependencies: ["ResonateNative", "ResonateLogic", "ResonateWidgetLinks"],
        path: "Sources/Resonate",
        swiftSettings: [
            .unsafeFlags([
                "-emit-const-values",
                "-Xfrontend", "-const-gather-protocols-file",
                "-Xfrontend", intentProtocols,
            ])
        ]
    ),
    .target(
        name: "ResonateWidgets",
        dependencies: ["ResonateWidgetLinks"],
        swiftSettings: [.unsafeFlags(["-application-extension"])],
        linkerSettings: [
            .unsafeFlags(["-Xlinker", "-application_extension", "-Xlinker", "-u", "-Xlinker", "_main"])
        ]
    ),
]

let package = Package(
    name: "Resonate",
    defaultLocalization: "en",
    platforms: [.iOS(.v26)],
    products: onTheHost ? [] : [
        .library(name: "Resonate", targets: ["Resonate"]),
        .library(name: "ResonateWidgets", targets: ["ResonateWidgets"]),
    ],
    targets: [
        .target(name: "ResonateWidgetLinks"),
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
            dependencies: ["ResonateLogic", "ResonateWidgetLinks"],
            path: "Tests/ResonateLogicTests"
        ),
    ] + (onTheHost ? [] : app)
)
