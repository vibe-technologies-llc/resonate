Name:           resonate
Version:        0.1.0
Release:        1%{?dist}
Summary:        High-fidelity music player with native PipeWire output
License:        AGPL-3.0-or-later
Source0:        %{name}-%{version}.tar.gz
ExclusiveArch:  x86_64 aarch64

%ifarch x86_64
%global rust_baseline x86-64
%else
%global rust_baseline generic
%endif

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
Requires:       pipewire >= 0.3.50
Requires:       vulkan-loader

%description
Resonate plays music through PipeWire with a native Wayland interface.

%prep
%autosetup

%build
export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=%{rust_baseline}"
cargo build --locked --release --workspace --bin resonate

%check
export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=%{rust_baseline}"
cargo test --locked --release --workspace --exclude resonate-ui --no-default-features

%install
generated=$(find target/release/build -name resonate.1 -printf '%h\n' -quit)
test -n "$generated"
install -Dm755 target/release/resonate %{buildroot}%{_bindir}/resonate
install -Dm644 packaging/resonate.desktop %{buildroot}%{_datadir}/applications/resonate.desktop
install -Dm644 packaging/resonate.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/resonate.svg
install -Dm644 packaging/org.resonate.Resonate.metainfo.xml %{buildroot}%{_datadir}/metainfo/org.resonate.Resonate.metainfo.xml
install -Dm644 -t %{buildroot}%{_mandir}/man1 "$generated"/*.1
install -Dm644 "$generated/resonate.bash" %{buildroot}%{_datadir}/bash-completion/completions/resonate
install -Dm644 "$generated/_resonate" %{buildroot}%{_datadir}/zsh/site-functions/_resonate
install -Dm644 "$generated/resonate.fish" %{buildroot}%{_datadir}/fish/vendor_completions.d/resonate.fish

%files
%license LICENSE
%doc README.md
%{_bindir}/resonate
%{_datadir}/applications/resonate.desktop
%{_datadir}/icons/hicolor/scalable/apps/resonate.svg
%{_datadir}/metainfo/org.resonate.Resonate.metainfo.xml
%{_mandir}/man1/resonate*.1*
%{_datadir}/bash-completion/completions/resonate
%{_datadir}/zsh/site-functions/_resonate
%{_datadir}/fish/vendor_completions.d/resonate.fish

%changelog
* Thu Sep 24 2026 Resonate - 0.1.0-1
- Add Fedora 44 RPM packaging
