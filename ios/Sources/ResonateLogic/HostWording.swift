#if !canImport(Darwin)
import Foundation

struct HostWording: Sendable {
    let strings: [String: String]
    let plurals: [String: Plural]

    struct Plural: Sendable {
        let format: String
        let forms: [String: Forms]
    }

    struct Forms: Sendable {
        let one: String
        let other: String
    }

    private static let table = "Localizable"

    init(folder: URL) {
        strings = Self.entries(of: (try? String(contentsOf: folder.appendingPathComponent(Self.table).appendingPathExtension("strings"), encoding: .utf8)) ?? "")
        plurals = Self.plurals(of: folder.appendingPathComponent(Self.table).appendingPathExtension("stringsdict"))
    }

    func said(_ key: String, _ arguments: [any CVarArg]) -> String {
        guard let plural = plurals[key] else {
            let format = strings[key] ?? key
            return arguments.isEmpty ? format : String(format: format, arguments: arguments)
        }
        return String(format: Self.chosen(plural, arguments), arguments: arguments)
    }

    private static let specifier = try! NSRegularExpression(
        pattern: #"%(?:(\d+)\$)?(#@(\w+)@|%|[-+\x200#]*\d*(?:\.\d+)?(?:ll|l|h|q)?[@dDiuUxXoOfFeEgGcsSp])"#
    )

    private static func chosen(_ plural: Plural, _ arguments: [any CVarArg]) -> String {
        var next = 1
        return positioned(plural.format, next: &next) { variable, place in
            guard let forms = plural.forms[variable] else { return "" }
            let form = isOne(place <= arguments.count ? arguments[place - 1] : nil) ? forms.one : forms.other
            var inner = place
            return positioned(form, next: &inner, fixed: place) { _, _ in "" }
        }
    }

    private static func positioned(
        _ format: String,
        next: inout Int,
        fixed: Int? = nil,
        variable: (String, Int) -> String
    ) -> String {
        let text = format as NSString
        var written = ""
        var from = 0
        for match in specifier.matches(in: format, range: NSRange(location: 0, length: text.length)) {
            written += text.substring(with: NSRange(location: from, length: match.range.location - from))
            from = match.range.location + match.range.length
            let body = text.substring(with: match.range(at: 2))
            if body == "%" {
                written += "%%"
                continue
            }
            let place: Int
            if let fixed {
                place = fixed
            } else if match.range(at: 1).location != NSNotFound {
                place = Int(text.substring(with: match.range(at: 1))) ?? next
            } else {
                place = next
                next += 1
            }
            if match.range(at: 3).location != NSNotFound {
                written += variable(text.substring(with: match.range(at: 3)), place)
            } else {
                written += "%\(place)$\(body)"
            }
        }
        written += text.substring(from: from)
        return written
    }

    private static func isOne(_ argument: (any CVarArg)?) -> Bool {
        switch argument {
        case let count as any BinaryInteger: Int(exactly: count) == 1
        case let count as Double: count == 1
        default: false
        }
    }

    private static func entries(of text: String) -> [String: String] {
        var found: [String: String] = [:]
        for line in text.split(separator: "\n") {
            let quoted = Self.quoted(String(line))
            if quoted.count == 2 {
                found[quoted[0]] = quoted[1]
            }
        }
        return found
    }

    private static func quoted(_ line: String) -> [String] {
        var parts: [String] = []
        var current = ""
        var inside = false
        var escaped = false
        for letter in line {
            if !inside {
                if letter == "\"" { inside = true }
                continue
            }
            if escaped {
                current.append(letter == "n" ? "\n" : letter == "t" ? "\t" : letter)
                escaped = false
            } else if letter == "\\" {
                escaped = true
            } else if letter == "\"" {
                parts.append(current)
                current = ""
                inside = false
            } else {
                current.append(letter)
            }
        }
        return parts
    }

    private static func plurals(of file: URL) -> [String: Plural] {
        guard let data = try? Data(contentsOf: file),
            let read = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any]
        else { return [:] }
        var found: [String: Plural] = [:]
        for (key, value) in read {
            guard let entry = value as? [String: Any], let format = entry["NSStringLocalizedFormatKey"] as? String else { continue }
            var forms: [String: Forms] = [:]
            for (name, spec) in entry {
                guard let spec = spec as? [String: Any], let one = spec["one"] as? String, let other = spec["other"] as? String else { continue }
                forms[name] = Forms(one: one, other: other)
            }
            found[key] = Plural(format: format, forms: forms)
        }
        return found
    }
}
#endif
