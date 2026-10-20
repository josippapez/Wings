// Wings' on-device describer. `describe --check` prints why the model is unavailable and exits 2, or exits 0
// when it is ready. Otherwise it reads {"instructions","prompt"} on stdin and prints the model's reply.
// Apple's on-device model only; Wings compiles this with swiftc and treats a missing compiler or model as "unavailable".
import Foundation
import FoundationModels

struct Input: Decodable { let instructions: String; let prompt: String }

func unavailable() -> String? {
  switch SystemLanguageModel.default.availability {
  case .available: return nil
  case .unavailable(.deviceNotEligible): return "This Mac doesn't support Apple Intelligence"
  case .unavailable(.appleIntelligenceNotEnabled): return "Apple Intelligence is turned off in System Settings"
  case .unavailable(.modelNotReady): return "Apple Intelligence is still downloading its model"
  case .unavailable: return "The on-device model isn't available"
  }
}

@main struct Describe {
  static func main() async {
    if let why = unavailable() {
      print(why)
      exit(2)
    }
    if CommandLine.arguments.contains("--check") { exit(0) }
    let data = FileHandle.standardInput.readDataToEndOfFile()
    guard let input = try? JSONDecoder().decode(Input.self, from: data) else { exit(1) }
    let session = LanguageModelSession(instructions: input.instructions)
    do {
      print(try await session.respond(to: input.prompt).content)
    } catch {
      FileHandle.standardError.write(Data("\(error)\n".utf8))
      exit(1)
    }
  }
}
