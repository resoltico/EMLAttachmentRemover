// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import Darwin
import Foundation
import Security

// Observe this caller only; never create or enter a different security session.
var session: SecuritySessionId = 0
var attributes: SessionAttributeBits = []
let status = SessionGetInfo(callerSecuritySession, &session, &attributes)
var facts: [String: Any] = ["status": status]
if status == errSecSuccess {
  facts["security_session_id"] = session
  facts["graphic_access"] = attributes.contains(.sessionHasGraphicAccess)
}
FileHandle.standardOutput.write(
  try JSONSerialization.data(withJSONObject: facts, options: [.sortedKeys]))
print()
if status != errSecSuccess { exit(1) }
