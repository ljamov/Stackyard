# Signing the installers

Unsigned installers work, but Windows shows "Windows protected your PC" and macOS refuses to open
the app until it is right-clicked and opened by hand. Signing removes those warnings. The
Installers workflow signs whenever the secrets below exist and builds unsigned installers when
they don't, so nothing breaks while they are missing. Each signed build checks its own signatures
and fails if anything came out unsigned.

Nothing here is needed for updates: those are signed with Stackyard's own update key
(see [updates.md](updates.md)) whether or not the installers are.

Secrets and variables are added on GitHub in the repository's **Settings › Secrets and variables
› Actions**: **New repository secret** for a secret, or the **Variables** tab and **New repository
variable** for a variable. The name must match exactly.

## macOS: Apple Developer ID

Costs $99 a year. Signing alone stops the "damaged or unverified developer" message; notarizing
(Apple scans the app and approves it) lets it open with a normal double click.

1. **Join the Apple Developer Program** at <https://developer.apple.com/programs/enroll/> with
   your Apple ID (it needs two-factor authentication). Enroll as an individual; approval can take
   a day or two.
2. **Make a certificate request.** It holds a private key that never leaves your computer. On
   Windows, open **Git Bash** (it comes with Git for Windows) and paste:

   ```sh
   cd ~/Desktop
   openssl req -new -newkey rsa:2048 -nodes -keyout stackyard-apple.key -out stackyard-apple.csr -subj "/CN=Stackyard/emailAddress=you@example.com"
   ```

   (On a Mac: **Keychain Access › Certificate Assistant › Request a Certificate From a
   Certificate Authority**, saved to disk.)
3. **Create the certificate.** At <https://developer.apple.com/account/resources/certificates/list>
   click **+**, choose **Developer ID Application**, pick the **G2 Sub-CA** option, upload
   `stackyard-apple.csr`, then **Download**. You get `developerID_application.cer`.
4. **Turn it into one file with a password.** In Git Bash, in the same folder, pick a password
   and paste (replace `choose-a-password`):

   ```sh
   openssl x509 -inform der -in developerID_application.cer -out developerID_application.pem
   openssl pkcs12 -export -legacy -inkey stackyard-apple.key -in developerID_application.pem -out stackyard-apple.p12 -passout pass:choose-a-password
   openssl base64 -A -in stackyard-apple.p12 > stackyard-apple-base64.txt
   ```

5. **Add the secrets:**
   - `APPLE_CERTIFICATE`: everything in `stackyard-apple-base64.txt` (open it in Notepad, select
     all, copy).
   - `APPLE_CERTIFICATE_PASSWORD`: the password from step 4.
6. **For notarizing, also add:**
   - `APPLE_ID`: the email of your Apple ID.
   - `APPLE_PASSWORD`: an app-specific password, made at <https://account.apple.com> under
     **Sign-In and Security › App-Specific Passwords** (not your normal password).
   - `APPLE_TEAM_ID`: the 10-character Team ID shown at <https://developer.apple.com/account>
     under **Membership details**.
7. Keep `stackyard-apple.key` and `stackyard-apple.p12` somewhere safe (a password manager) and
   delete the other files.

The certificate's name is read from the certificate itself. If a build says it can't read it, add
the `APPLE_SIGNING_IDENTITY` secret with the name exactly as Keychain Access shows it, such as
`Developer ID Application: Your Name (TEAMID1234)`.

## Windows: Azure Artifact Signing

Microsoft's own signing service, about $10 a month, and the one Microsoft recommends. Windows'
SmartScreen still warns on the first downloads of a new signed app and stops once the signature
has built a reputation; signing every release with the same identity is what builds it.

Who can use it: companies in the USA, Canada, the EU and the UK, and individuals in the USA and
Canada. Elsewhere, see [Other ways to sign for Windows](#other-ways-to-sign-for-windows).

1. **Create an Azure account** at <https://azure.microsoft.com/free> and sign in to the
   portal at <https://portal.azure.com>.
2. **Allow the service:** search for **Subscriptions**, open yours, then **Resource providers**,
   find `Microsoft.CodeSigning` and click **Register**.
3. **Create the signing account:** search for **Artifact Signing Accounts**, click **Create**,
   make a new resource group (any name, such as `stackyard`), give the account a name (such as
   `stackyard`), pick a region near you and the **Basic** tier, then **Review + create**.
   Write down the account name and the region's **endpoint** shown on the account's overview
   page, such as `https://weu.codesigning.azure.net/`.
4. **Give yourself the right to verify:** in the account, **Access control (IAM) › Add › Add role
   assignment**, choose **Artifact Signing Identity Verifier**, and add yourself.
5. **Verify your identity:** in the account, **Identity validations › New identity › Public**, and
   fill in the details exactly as on your company papers or ID. Microsoft checks them; it can
   take from a day to a few weeks, and sends emails along the way.
6. **Create a certificate profile:** once the identity is **Completed**, **Certificate profiles ›
   Create › Public Trust**, name it (such as `stackyard`) and pick the verified identity. Write
   down the profile name.
7. **Create a sign-in for GitHub:** search for **App registrations › New registration**, name it
   `stackyard-github`, click **Register**. On its page write down the **Application (client) ID**
   and the **Directory (tenant) ID**. Then **Certificates & secrets › New client secret**, and
   copy its **Value** right away (it is shown once).
8. **Let that sign-in sign:** back in the signing account, **Access control (IAM) › Add › Add role
   assignment**, choose **Artifact Signing Certificate Profile Signer**, and add `stackyard-github`.
9. **Add the secrets:** `AZURE_TENANT_ID` (Directory ID), `AZURE_CLIENT_ID` (Application ID) and
   `AZURE_CLIENT_SECRET` (the secret's Value).
10. **Add the variables:** `AZURE_SIGNING_ENDPOINT` (the endpoint from step 3),
    `AZURE_SIGNING_ACCOUNT` (the account name) and `AZURE_SIGNING_PROFILE` (the profile name).

The client secret expires (two years at most); make a new one in step 7 and replace
`AZURE_CLIENT_SECRET` before then.

### Other ways to sign for Windows

- **SignPath Foundation** signs open-source projects for free after reviewing them
  (<https://signpath.org>). It signs through its own service rather than inside this workflow, so
  using it means changing the workflow once the project is accepted.
- **A code signing certificate from a certificate authority** (DigiCert, Sectigo, SSL.com,
  Certum), roughly $150 to $300 a year, available to individuals anywhere. Since 2023 the key must
  live on a hardware token or in the authority's cloud, so pick one with cloud signing that works
  from GitHub Actions; that also needs a small change to this workflow.

## After adding secrets

Run the Installers workflow by hand (**Actions › Installers › Run workflow** on master, without
"release" ticked). The summary of each run says whether it signed, and the job fails with a clear
message if a secret is wrong. Once a release is signed, its notes stop telling people how to get
past the warning.
