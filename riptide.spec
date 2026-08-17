# SPDX-FileCopyrightText: 2026 Nicolás Rodríguez Álvarez
# SPDX-License-Identifier: GPL-3.0-or-later

# cargo build --release carries no debug info, so the default debuginfo
# extraction produces an empty package and fails the build.
%global debug_package %{nil}

Name:           riptide
Version:        1.1.0
Release:        1%{?dist}
Summary:        Terminal UI music player for Tidal
License:        GPL-3.0-or-later
URL:            https://github.com/fezzik-the-giant/riptide
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/%{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  pkgconfig(openssl)
BuildRequires:  chafa-devel

# mpv is launched as a subprocess over its JSON IPC socket, so nothing links
# against it and RPM's automatic dependency generator cannot see it. The library
# dependencies (libchafa, libssl, glib) are read from the ELF and need no entry.
Requires:       mpv

%description
Riptide is a terminal-based music player for Tidal with a TUI interface built in
Rust (ratatui), driving mpv over its JSON IPC socket for playback.

%prep
%autosetup -n %{name}-%{version}

# Deliberately not cargo-rpm-macros. Its prep macro writes "net offline = true"
# unconditionally and repoints cargo at Fedora's local crate registry, which
# would require all 478 transitive dependencies to be packaged as crate() RPMs.
# This targets a COPR user repo instead, where "Enable internet access during
# builds" lets cargo fetch from crates.io against the committed Cargo.lock.
# Submitting to Fedora proper would need cargo vendor and a vendored tarball.

%build
cargo build --release --locked

%install
install -D -p -m 0755 target/release/%{name} %{buildroot}%{_bindir}/%{name}

%check
cargo test --release --locked

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/%{name}

%changelog
* Mon Aug 17 2026 Ryan Cohan <noreply@github.com> - 1.1.0-1
- Point the package at upstream rather than a fork
- Build with plain cargo so the spec works on a network-enabled COPR builder

* Fri Aug 14 2026 Nicolás Rodríguez Álvarez <noreply@github.com> - 1.0.0-1
- Initial COPR packaging
