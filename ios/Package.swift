// swift-tools-version: 6.2

import Foundation
import PackageDescription

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let native = root.appendingPathComponent("Generated/lib").path
let intentProtocols = root.appendingPathComponent("Resonate/AppIntentsProtocols.json").path

let package = Package(
    name: "Resonate",
    platforms: [.iOS(.v26)],
    products: [
        .library(name: "Resonate", targets: ["Resonate"])
    ],
    targets: [
        .systemLibrary(name: "ResonateNativeFFI", path: "Generated/ResonateNativeFFI"),
        .target(
            name: "ResonateNative",
            dependencies: ["ResonateNativeFFI"],
            path: "Generated/ResonateNative",
            linkerSettings: [
                .unsafeFlags(["-L", native]),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
        .target(
            name: "Resonate",
            dependencies: ["ResonateNative"],
            path: "Sources/Resonate",
            swiftSettings: [
                .unsafeFlags([
                    "-emit-const-values",
                    "-Xfrontend", "-const-gather-protocols-file",
                    "-Xfrontend", intentProtocols,
                ])
            ]
        ),
    ]
)
