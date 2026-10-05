import Darwin
import Foundation

func launchExplanation(_ error: any Error) -> String {
  let native = error as NSError
  if native.domain == "EMLBundledRuntime" {
    return
      "The app’s private Python runtime is missing or damaged. Download and install a fresh copy of the app. No files were processed by this launch."
  }
  if native.domain == NSPOSIXErrorDomain && native.code == Int(E2BIG) {
    return
      "This file selection is too large to launch as one batch. Select fewer files and try again. No files were processed by this launch."
  }
  return
    "Processing could not start. Check the application installation and runtime configuration. Details contain the launch error."
}
