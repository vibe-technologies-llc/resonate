Name:           resonate
Version:        0.11.0
Release:        1%{?dist}
Summary:        High-fidelity music player with native PipeWire output
License:        AGPL-3.0-or-later
Source0:        %{name}-%{version}.tar.gz
ExclusiveArch:  x86_64

%global rust_baseline x86-64-v3
%global debug_package %{nil}

BuildRequires:  cargo
BuildRequires:  clang
BuildRequires:  fontconfig-devel
BuildRequires:  freetype-devel
BuildRequires:  gcc
BuildRequires:  libxkbcommon-devel
BuildRequires:  libxkbcommon-x11-devel
BuildRequires:  pipewire-devel >= 0.3.50
BuildRequires:  pkgconf-pkg-config
BuildRequires:  rust
BuildRequires:  vulkan-headers
BuildRequires:  vulkan-loader-devel
BuildRequires:  wayland-devel
BuildRequires:  webkit2gtk4.1-devel
Requires:       pipewire >= 0.3.50
Requires:       vulkan-loader
Requires:       rsms-inter-fonts
Requires:       jetbrains-mono-fonts
Suggests:       yt-dlp
Suggests:       deno
Recommends:     resonate-signin

%description
Resonate plays music through PipeWire with a native Wayland interface.

%package -n resonate-signin
Summary:        Resonate's YouTube Music sign-in window, on WebKitGTK

%description -n resonate-signin
A window of its own that signs in to YouTube Music on Google's page and keeps
the account's cookies for Resonate, so no browser of the listener's is read.

%prep
%autosetup

%build
export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=%{rust_baseline}"
cargo build --locked --release --package resonate --bin resonate
cargo build --locked --release --package resonate-signin

%install
built=${CARGO_TARGET_DIR:-target}/release
generated=$(find "$built/build" -name resonate.1 -printf '%h\n' -quit)
test -n "$generated"
install -Dm755 "$built/resonate" %{buildroot}%{_bindir}/resonate
install -Dm755 "$built/resonate-signin" %{buildroot}%{_bindir}/resonate-signin
install -Dm644 packaging/resonate.desktop %{buildroot}%{_datadir}/applications/resonate.desktop
install -Dm644 packaging/resonate.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/resonate.svg
install -Dm644 packaging/org.resonate.Resonate.metainfo.xml %{buildroot}%{_datadir}/metainfo/org.resonate.Resonate.metainfo.xml
install -Dm644 -t %{buildroot}%{_mandir}/man1 "$generated"/*.1
install -Dm644 -t %{buildroot}%{_mandir}/man5 packaging/resonate-config.toml.5
install -Dm644 "$generated/resonate.bash" %{buildroot}%{_datadir}/bash-completion/completions/resonate
install -Dm644 "$generated/_resonate" %{buildroot}%{_datadir}/zsh/site-functions/_resonate
install -Dm644 "$generated/resonate.fish" %{buildroot}%{_datadir}/fish/vendor_completions.d/resonate.fish

%files -n resonate-signin
%license LICENSE
%{_bindir}/resonate-signin

%files
%license LICENSE
%doc README.md COPYRIGHT
%{_bindir}/resonate
%{_datadir}/applications/resonate.desktop
%{_datadir}/icons/hicolor/scalable/apps/resonate.svg
%{_datadir}/metainfo/org.resonate.Resonate.metainfo.xml
%{_mandir}/man1/resonate*.1*
%{_mandir}/man5/resonate-config.toml.5*
%{_datadir}/bash-completion/completions/resonate
%{_datadir}/zsh/site-functions/_resonate
%{_datadir}/fish/vendor_completions.d/resonate.fish

%changelog
* Sat Nov 14 2026 Resonate - 0.11.0-1
- Features; see the GitHub release for the list
* Fri Nov 13 2026 Resonate - 0.10.0-1
- Features; see the GitHub release for the list
* Thu Nov 12 2026 Resonate - 0.9.0-1
- Features; see the GitHub release for the list
* Tue Nov 10 2026 Resonate - 0.8.0-1
- Features; see the GitHub release for the list
* Mon Nov 09 2026 Resonate - 0.7.0-1
- Features; see the GitHub release for the list
* Sat Nov 07 2026 Resonate - 0.6.0-1
- Features; see the GitHub release for the list
* Thu Nov 05 2026 Resonate - 0.5.0-1
- Features; see the GitHub release for the list
* Sun Nov 01 2026 Resonate - 0.4.0-1
- Features; see the GitHub release for the list
* Fri Oct 30 2026 Resonate - 0.3.0-1
- Features; see the GitHub release for the list
* Tue Oct 27 2026 Resonate - 0.2.0-1
- Features; see the GitHub release for the list
* Sun Oct 25 2026 Resonate - 0.1.1-1
- Fixes only; see the GitHub release for the list
* Thu Sep 24 2026 Resonate - 0.1.0-1
- Add Fedora 44 RPM packaging
