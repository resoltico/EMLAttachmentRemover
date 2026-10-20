// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import Foundation
import SwiftParser
import SwiftSyntax

// Branch complexity is independent of Clippy's cognitive-complexity algorithm.
// Each body starts at one; conditionals, loops, catches, nondefault switch cases,
// ternaries, additional comma conditions and &&/|| add one. Nested callable bodies
// are measured separately.
private func isCallable(_ node: Syntax) -> Bool {
  node.is(FunctionDeclSyntax.self) || node.is(InitializerDeclSyntax.self)
    || node.is(DeinitializerDeclSyntax.self) || node.is(AccessorDeclSyntax.self)
    || node.is(ClosureExprSyntax.self)
}

private final class BodyMetrics: SyntaxAnyVisitor {
  let locations: SourceLocationConverter
  var codeLines = Set<Int>()
  var branches = 1

  init(locations: SourceLocationConverter) {
    self.locations = locations
    super.init(viewMode: .sourceAccurate)
  }

  override func visitAny(_ node: Syntax) -> SyntaxVisitorContinueKind {
    if isCallable(node) { return .skipChildren }
    switch node.kind {
    case .ifExpr, .guardStmt, .forStmt, .whileStmt, .repeatStmt, .catchClause,
      .ternaryExpr, .unresolvedTernaryExpr:
      branches += 1
      if let conditional = node.as(IfExprSyntax.self) {
        branches += max(0, conditional.conditions.count - 1)
      }
      if let guardStatement = node.as(GuardStmtSyntax.self) {
        branches += max(0, guardStatement.conditions.count - 1)
      }
    case .switchCase:
      if node.as(SwitchCaseSyntax.self)!.label.is(SwitchCaseLabelSyntax.self) { branches += 1 }
    default: break
    }
    if let token = node.as(TokenSyntax.self), token.presence == .present {
      let start = token.positionAfterSkippingLeadingTrivia
      let end = token.endPositionBeforeTrailingTrivia
      if end.utf8Offset > start.utf8Offset {
        let first = locations.location(for: start).line
        let last = locations.location(for: AbsolutePosition(utf8Offset: end.utf8Offset - 1)).line
        codeLines.formUnion(first...last)
      }
      if case .binaryOperator(let spelling) = token.tokenKind,
        spelling == "&&" || spelling == "||"
      {
        branches += 1
      }
    }
    return .visitChildren
  }
}

private final class SourcePolicy: SyntaxVisitor {
  let file: String
  let locations: SourceLocationConverter
  let maximumLines: Int
  let maximumBranches: Int
  let maximumParameters: Int
  var failures: [String] = []
  var bodies = 0

  init(file: String, tree: SourceFileSyntax, limits: [Int]) {
    self.file = file
    locations = SourceLocationConverter(fileName: file, tree: tree)
    maximumLines = limits[0]
    maximumBranches = limits[1]
    maximumParameters = limits[2]
    super.init(viewMode: .sourceAccurate)
  }

  private func check(_ body: some SyntaxProtocol, parameters: Int = 0) {
    let metrics = BodyMetrics(locations: locations)
    metrics.walk(body)
    bodies += 1
    let line = locations.location(for: body.positionAfterSkippingLeadingTrivia).line
    for (label, actual, maximum) in [
      ("code lines", metrics.codeLines.count, maximumLines),
      ("branch complexity", metrics.branches, maximumBranches),
      ("parameters", parameters, maximumParameters),
    ] {
      if actual > maximum {
        failures.append("\(file):\(line): \(label) \(actual) exceeds \(maximum)")
      }
    }
  }

  override func visit(_ node: FunctionDeclSyntax) -> SyntaxVisitorContinueKind {
    if let body = node.body {
      check(body, parameters: node.signature.parameterClause.parameters.count)
    }
    return .visitChildren
  }
  override func visit(_ node: InitializerDeclSyntax) -> SyntaxVisitorContinueKind {
    if let body = node.body {
      check(body, parameters: node.signature.parameterClause.parameters.count)
    }
    return .visitChildren
  }
  override func visit(_ node: DeinitializerDeclSyntax) -> SyntaxVisitorContinueKind {
    if let body = node.body { check(body) }
    return .visitChildren
  }
  override func visit(_ node: AccessorDeclSyntax) -> SyntaxVisitorContinueKind {
    if let body = node.body { check(body) }
    return .visitChildren
  }
  override func visit(_ node: AccessorBlockSyntax) -> SyntaxVisitorContinueKind {
    if let body = node.accessors.as(CodeBlockItemListSyntax.self) { check(body) }
    return .visitChildren
  }
  override func visit(_ node: ClosureExprSyntax) -> SyntaxVisitorContinueKind {
    let parameters = node.signature?.parameterClause
    let count =
      parameters?.as(ClosureParameterClauseSyntax.self)?.parameters.count
      ?? parameters?.as(ClosureShorthandParameterListSyntax.self)?.count ?? 0
    check(node.statements, parameters: count)
    return .visitChildren
  }
  override func visit(_ node: SubscriptDeclSyntax) -> SyntaxVisitorContinueKind {
    let count = node.parameterClause.parameters.count
    if count > maximumParameters {
      let line = locations.location(for: node.positionAfterSkippingLeadingTrivia).line
      failures.append("\(file):\(line): parameters \(count) exceeds \(maximumParameters)")
    }
    return .visitChildren
  }
  override func visit(_ token: TokenSyntax) -> SyntaxVisitorContinueKind {
    if token.leadingTrivia.description.contains("swift-format-ignore")
      || token.trailingTrivia.description.contains("swift-format-ignore")
    {
      let line = locations.location(for: token.positionAfterSkippingLeadingTrivia).line
      failures.append("\(file):\(line): unregistered Swift formatter suppression")
    }
    return .visitChildren
  }
}

@main
private struct CheckSourcePolicy {
  static func main() throws {
    let arguments = Array(CommandLine.arguments.dropFirst())
    guard arguments.count >= 4,
      let lines = Int(arguments[0]), let branches = Int(arguments[1]),
      let parameters = Int(arguments[2]), min(lines, branches, parameters) > 0
    else {
      throw NSError(
        domain: "SourcePolicy requires positive line/branch/parameter limits and source paths",
        code: 1)
    }
    var failures: [String] = []
    var bodies = 0
    for file in arguments.dropFirst(3) {
      let source = try String(contentsOfFile: file, encoding: .utf8)
      let tree = Parser.parse(source: source)
      if tree.hasError {
        failures.append("\(file): SwiftParser rejected source; no structural result is admitted")
        continue
      }
      let visitor = SourcePolicy(file: file, tree: tree, limits: [lines, branches, parameters])
      visitor.walk(tree)
      failures += visitor.failures
      bodies += visitor.bodies
    }
    guard failures.isEmpty else {
      FileHandle.standardError.write(Data((failures.joined(separator: "\n") + "\n").utf8))
      exit(1)
    }
    guard bodies > 0 else { throw NSError(domain: "Swift callable discovery is empty", code: 1) }
    print(
      "Swift structural policy: \(arguments.count - 3) files, \(bodies) callable bodies checked")
  }
}
