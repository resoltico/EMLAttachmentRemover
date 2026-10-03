import Darwin
import Foundation

func validateProcessArguments(_ arguments: [String]) throws {
  // Foundation raises an Objective-C exception beyond this count, before NSError delivery.
  // The launcher script consumes one argument; file paths consume the remaining slots.
  if arguments.count > 4096 {
    throw NSError(
      domain: NSPOSIXErrorDomain, code: Int(E2BIG),
      userInfo: [NSLocalizedDescriptionKey: "The process launcher accepts at most 4,096 arguments."]
    )
  }
}
