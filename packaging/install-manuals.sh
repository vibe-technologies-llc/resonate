set -eu

generated=$1
destination=$2
config_page=resonate-config.toml.5

install_manuals() {
    mkdir -p "$3/man1" "$3/man5"
    install -m 644 "$1"/*.1 "$3/man1/"
    install -m 644 "$2" "$3/man5/"
}

install_manuals "$generated" "packaging/$config_page" "$destination"
for config in packaging/*/"$config_page"; do
    locale=${config#packaging/}
    locale=${locale%/*}
    install_manuals "$generated/$locale" "$config" "$destination/$locale"
done
