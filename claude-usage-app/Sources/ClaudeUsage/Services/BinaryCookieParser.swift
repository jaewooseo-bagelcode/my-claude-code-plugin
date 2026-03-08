import Foundation

/// Parses Apple Binary Cookies (.binarycookies) format files.
/// Used to extract session cookies from Orion browser profiles.
struct BinaryCookie {
    let domain: String
    let name: String
    let value: String
    let path: String
    let isSecure: Bool
    let isHttpOnly: Bool
    let expiry: Date?
}

enum BinaryCookieParserError: Error {
    case invalidMagic, invalidData, fileNotFound
}

struct BinaryCookieParser {

    static func parse(fileAt path: String) throws -> [BinaryCookie] {
        guard FileManager.default.fileExists(atPath: path) else {
            throw BinaryCookieParserError.fileNotFound
        }
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        return try parse(data: data)
    }

    static func parse(data: Data) throws -> [BinaryCookie] {
        guard data.count >= 8,
              String(data: data[0..<4], encoding: .ascii) == "cook"
        else { throw BinaryCookieParserError.invalidMagic }

        let numPages = readBE32(data, at: 4)
        var pageSizes: [Int] = []
        for i in 0..<Int(numPages) {
            pageSizes.append(Int(readBE32(data, at: 8 + i * 4)))
        }

        var cookies: [BinaryCookie] = []
        var offset = 8 + Int(numPages) * 4

        for size in pageSizes {
            guard offset + size <= data.count else { break }
            let pageData = Data(data[offset..<(offset + size)])
            cookies.append(contentsOf: parsePage(pageData))
            offset += size
        }

        return cookies
    }

    // MARK: - Page parsing

    private static func parsePage(_ page: Data) -> [BinaryCookie] {
        guard page.count >= 8 else { return [] }
        // page[0..3] = 0x00000100 header
        let numCookies = Int(readLE32(page, at: 4))
        guard page.count >= 8 + numCookies * 4 else { return [] }

        var cookies: [BinaryCookie] = []
        for i in 0..<numCookies {
            let cookieOffset = Int(readLE32(page, at: 8 + i * 4))
            if let cookie = parseCookie(page, at: cookieOffset) {
                cookies.append(cookie)
            }
        }
        return cookies
    }

    private static func parseCookie(_ data: Data, at start: Int) -> BinaryCookie? {
        // Cookie record: 4(size) + 4(flags) + 4(unknown) + 4(urlOff) + 4(nameOff) + 4(pathOff) + 4(valueOff) + 8(comment) + 8(expiry) + 8(creation) = 52 bytes header
        guard start + 52 <= data.count else { return nil }

        let flags = readLE32(data, at: start + 4)
        let domainOff = Int(readLE32(data, at: start + 16))
        let nameOff   = Int(readLE32(data, at: start + 20))
        let pathOff   = Int(readLE32(data, at: start + 24))
        let valueOff  = Int(readLE32(data, at: start + 28))
        let expiryRaw = readF64LE(data, at: start + 40)

        let domain = readCString(data, at: start + domainOff)
        let name   = readCString(data, at: start + nameOff)
        let path   = readCString(data, at: start + pathOff)
        let value  = readCString(data, at: start + valueOff)

        let expiry = expiryRaw > 0 ? Date(timeIntervalSinceReferenceDate: expiryRaw) : nil

        return BinaryCookie(
            domain: domain, name: name, value: value, path: path,
            isSecure: (flags & 1) != 0,
            isHttpOnly: (flags & 4) != 0,
            expiry: expiry
        )
    }

    // MARK: - Binary read helpers

    private static func readBE32(_ d: Data, at off: Int) -> UInt32 {
        guard off + 4 <= d.count else { return 0 }
        return UInt32(d[off]) << 24 | UInt32(d[off+1]) << 16 | UInt32(d[off+2]) << 8 | UInt32(d[off+3])
    }

    private static func readLE32(_ d: Data, at off: Int) -> UInt32 {
        guard off + 4 <= d.count else { return 0 }
        return UInt32(d[off]) | UInt32(d[off+1]) << 8 | UInt32(d[off+2]) << 16 | UInt32(d[off+3]) << 24
    }

    private static func readF64LE(_ d: Data, at off: Int) -> Double {
        guard off + 8 <= d.count else { return 0 }
        var value: Double = 0
        _ = withUnsafeMutableBytes(of: &value) { ptr in
            d.copyBytes(to: ptr, from: off..<(off + 8))
        }
        return value
    }

    private static func readCString(_ d: Data, at off: Int) -> String {
        guard off >= 0, off < d.count else { return "" }
        var end = off
        while end < d.count && d[end] != 0 { end += 1 }
        return String(data: d[off..<end], encoding: .utf8) ?? ""
    }
}
