Name:           resonate-server
Version:        0.1.1
Release:        1%{?dist}
Summary:        Sync server for Resonate: keeps sealed records and introduces devices
License:        AGPL-3.0-or-later
Source0:        resonate-%{version}.tar.gz
ExclusiveArch:  x86_64

%global rust_baseline x86-64-v3
%global debug_package %{nil}

BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  pkgconf-pkg-config
BuildRequires:  rust
BuildRequires:  systemd-rpm-macros
%{?systemd_requires}

%description
The Resonate sync server keeps records its devices sealed under keys it never
holds, and introduces devices that share a pairing code. It runs as a
hardened systemd service with a dynamic user.

%prep
%autosetup -n resonate-%{version}

%build
export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=%{rust_baseline}"
cargo build --locked --release --package resonate-server

%install
install -Dm755 "${CARGO_TARGET_DIR:-target}/release/resonate-server" %{buildroot}%{_bindir}/resonate-server
install -Dm644 packaging/server/resonate-server.service %{buildroot}%{_unitdir}/resonate-server.service
install -Dm644 packaging/server/resonate-server.env %{buildroot}%{_sysconfdir}/resonate-server/resonate-server.env

%post
%systemd_post resonate-server.service

%preun
%systemd_preun resonate-server.service

%postun
%systemd_postun_with_restart resonate-server.service

%files
%license LICENSE
%doc README.md COPYRIGHT
%{_bindir}/resonate-server
%{_unitdir}/resonate-server.service
%dir %{_sysconfdir}/resonate-server
%config(noreplace) %{_sysconfdir}/resonate-server/resonate-server.env

%changelog
* Sun Oct 25 2026 Resonate - 0.1.1-1
- Fixes only; see the GitHub release for the list
* Tue Oct 20 2026 Resonate - 0.1.0-1
- Add Fedora RPM packaging for the sync server
