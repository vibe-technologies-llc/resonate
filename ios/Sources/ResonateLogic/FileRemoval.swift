import Foundation

public enum FileRemoval {
    public static func removed(_ file: URL, under folder: URL) -> Bool {
        do {
            try FileManager.default.removeItem(at: file)
            return true
        } catch {
            return isGone(after: error, folderReachable: reachable(folder))
        }
    }

    public static func isGone(after failure: any Swift.Error, folderReachable: Bool) -> Bool {
        folderReachable && saysNoSuchFile(failure as NSError)
    }

    static func reachable(_ folder: URL) -> Bool {
        var directory: ObjCBool = false
        return FileManager.default.fileExists(atPath: folder.path, isDirectory: &directory) && directory.boolValue
    }

    private static func saysNoSuchFile(_ failure: NSError) -> Bool {
        switch failure.domain {
        case NSCocoaErrorDomain where failure.code == CocoaError.Code.fileNoSuchFile.rawValue: true
        case NSPOSIXErrorDomain where failure.code == Int(POSIXErrorCode.ENOENT.rawValue): true
        default: (failure.userInfo[NSUnderlyingErrorKey] as? NSError).map(saysNoSuchFile) ?? false
        }
    }
}
