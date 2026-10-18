// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import Foundation

#if PICKER_MARKER_TESTS
  @main
  struct PickerDiagnosticOutputTests {
    static func main() throws {
      let bytes = Data("prefix EML_NATIVE_PICKER_REQUESTED suffix".utf8)
      for split in 0...bytes.count {
        let marker = PickerDiagnosticOutput()
        precondition(!marker.take())
        marker.observe(bytes.prefix(split))
        let first = marker.take()
        marker.observe(bytes.suffix(bytes.count - split))
        let second = marker.take()
        precondition(first != second, "Lost or duplicated marker across split \(split)")
        precondition(!marker.take(), "Replayed an already consumed marker")
      }
      let rejected = PickerDiagnosticOutput()
      rejected.observe(Data("EML_NATIVE_PICKER_REQUEST suffix".utf8))
      precondition(!rejected.take(), "Accepted an incomplete marker")
      precondition(!rejected.hasEnded())
      rejected.failRelay("first read failure")
      rejected.failRelay("later write failure")
      rejected.finish()
      precondition(rejected.hasEnded())
      precondition(rejected.observedRelayFailure() == "first read failure")
      try FileHandle.standardOutput.write(
        contentsOf: Data(
          "All marker split boundaries, rejection and EOF/error checks passed\n".utf8))
    }
  }
#endif
