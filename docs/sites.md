# Sites, local domains and HTTPS

How Stackyard serves your projects at addresses like `https://myapp.stack`.

## Sites

Add a site by hand, or park a folder (`stackyard sites park ~/Code`) so every project folder in it
becomes a site named after the folder. Stackyard looks at each project to decide how to serve it:

- **PHP**: Laravel, Symfony, CodeIgniter, Magento, CakePHP, Craft CMS, Yii, Drupal, WordPress
  (including Bedrock), or any folder with an `index.php`. The right public folder is served, through
  `php-fpm` on macOS and Linux and `php-cgi` on Windows, one process per PHP version in use.
  Each version has its own `php.ini` in `config/php/<version>/`, created once with development
  defaults and then left to you.
- **Node**: a project with a `dev` or `start` script is proxied to its dev server. The port comes
  from the script (`--port 4000`) or the framework's usual one (Vite 5173, Next.js 3000, Astro 4321…).
- **Static files**: anything else, from `public/` or `dist/` when the page lives there.

Every site answers at `https://<name><ending>` (`.stack` by default) and at `<name>.localhost`, which
browsers send to your own computer, so sites work before setup.

## Local domains and HTTPS

`stackyard system setup` (or **Set up** on the Sites screen) asks for your password once and:

| | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Local names | `/etc/resolver/<ending>` pointing at Stackyard's DNS server on 127.0.0.1:53535 | A systemd-resolved drop-in doing the same (the hosts file without systemd-resolved) | A marked block in the hosts file |
| HTTPS | Trusts Caddy's local root certificate in the System keychain | Adds it to the system certificates (and Chrome's list when `certutil` is installed) | Adds it to the trusted root certificates |
| Ports 80 and 443 | Already allowed | Lets programs use ports from 80 (`/etc/sysctl.d/50-stackyard.conf`) | Already allowed |

Only Stackyard's domain endings go to its DNS server; everything else resolves as before. The DNS
server answers `127.0.0.1` for names under those endings and refuses everything else. When ports
80 and 443 are taken, the web server uses 8080 and 8443 instead. `stackyard system setup --print`
shows the exact commands first. With the hosts file, adding a site asks for setup again.

## Domain endings

New sites end in `.stack` by default. Change it in the app (Sites › Domain ending) or with
`stackyard tld .test`. If `.stack` ever becomes a public domain ending, the default moves to `.test`,
which is reserved for testing. `.local` works but is not recommended because Bonjour/mDNS uses it.

---

[← Back to the README](../README.md)
