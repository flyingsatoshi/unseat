# Windows distribution and antivirus review

Unseat is a local sitting timer. The portable executable runs from the user's chosen folder with an `asInvoker`, `uiAccess=false` manifest. It does not require administrator rights. It no longer copies itself into AppData, relaunches a copy, or creates Start Menu shortcuts on launch. Launch with Windows defaults to off; changing that setting explicitly updates only the current user's Unseat Run entry. Existing preferences and Run entries are preserved.

The GUI uses ordinary Win32 windows, Direct2D drawing, a notification-area icon, last-input age, lock notifications, and power notifications. Last-input age does not capture keystrokes. Settings and timer checkpoints are local JSON. There is no network client, downloaded payload, process injection, executable packer, or antivirus exclusion in the application code.

## Build and sign releases

Use a Windows build environment with Rust, the MSVC C++ build tools, and the Windows SDK. CI uses the lockfile and runs both library and Windows host tests. GNU builds additionally require a working MinGW resource compiler. The icon, package-version metadata, and least-privilege manifest are required build inputs; missing resource tooling is a build error.

```powershell
cargo test --locked --all-targets
cargo build --locked --release --features gui
(Get-Item target/release/unseat.exe).VersionInfo | Format-List ProductName,FileDescription,FileVersion,OriginalFilename
```

Sign the final executable with the project's genuine, publicly trusted code-signing identity or approved signing service. Do not substitute a self-signed identity as evidence of public trust. For a certificate already provisioned in the current user's certificate store, use the actual thumbprint and the issuer's RFC 3161 timestamp endpoint:

```powershell
signtool sign /sha1 <certificate-thumbprint> /fd SHA256 /tr <timestamp-url> /td SHA256 target/release/unseat.exe
signtool verify /pa /all /v target/release/unseat.exe
Get-FileHash target/release/unseat.exe -Algorithm SHA256
```

Verify each command's result before continuing. Sign after building; distribute and submit the exact signed bytes and their SHA-256. Never modify the executable after signing. These commands require a provisioned signing identity; the repository does not contain a certificate or private key. Signing establishes publisher identity and integrity, not proof of harmlessness or a guaranteed antivirus exemption. See [Microsoft SignTool documentation](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool) and the [Microsoft software developer FAQ](https://learn.microsoft.com/en-us/defender-xdr/developer-faq).

## Investigate a detection

Record the exact detection name, module, timestamp, executable path, SHA-256, signature status, app version, antivirus version, and reproduction steps. The September 12 screenshot identifies Bitdefender Advanced Threat Defense and `unseat.exe`, but does not identify the triggering behavior. The older expanded detection points to `target/release`, which the previous self-relocation code treated as a development path and did not copy. Self-relocation therefore does not explain that specific event by itself.

Removal of unsolicited startup registration and self-relocation is an improvement in installation behavior, not confirmation that Bitdefender's diagnosis was a false positive. Test launch, settings save, explicit autostart opt-in/out, timer completion, and sleep/wake with protection enabled in a suitable build/test environment. If detection persists, submit the exact release binary and reproduction details for [Bitdefender's false-positive review](https://www.bitdefender.com/consumer/support/answer/29358/). A submission shares that binary with the vendor; it is a separate release-owner action. Await its verdict before claiming the detection is resolved.

Separately, Windows Application Control can prevent unsigned test or dependency-build executables from starting (`os error 4551`). Check `Microsoft-Windows-CodeIntegrity/Operational` for the matching path and policy. This is not a Rust assertion failure or proof that Bitdefender triggered. Run builds/tests in an approved development environment; source edits to the timer cannot override a system signing policy. No test pass or antivirus clearance should be reported when execution was blocked.
