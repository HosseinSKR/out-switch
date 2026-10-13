# Security policy

Out-Switch is pre-release software. No version is currently certified for production networking.

Report suspected vulnerabilities privately to **hosseinshakeri01@icloud.com**. Include the version, Windows build, reproduction steps, impact and a minimal example. Do not include passwords, private traffic or unrelated personal data. A response time is not guaranteed.

The service runs as LocalSystem when installed. Its control pipe permits the installation owner, administrators and SYSTEM, rejects remote clients, and checks the server executable path. The installer protects service data with an explicit ACL. Any change to these boundaries needs security review.

Do not publish an exploit or sensitive log in a public issue while coordinating a report. Driver signing and compatibility checks must not be bypassed to make a release appear functional.
