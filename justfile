# niri-cosmic-applets: COSMIC panel applets that talk to the niri compositor.
# Installs to ~/.local by default (no root needed). System-wide: `sudo just prefix=/usr install`.

prefix := env_var_or_default('PREFIX', env_var('HOME') + '/.local')
bindir := prefix / 'bin'
appdir := prefix / 'share/applications'

default: build

build:
    cargo build --release --workspace

install: build
    install -Dm0755 target/release/cosmic-applet-input-sources {{bindir}}/cosmic-applet-input-sources
    install -Dm0755 target/release/cosmic-applet-workspaces {{bindir}}/cosmic-applet-workspaces
    install -Dm0755 target/release/cosmic-app-list {{bindir}}/cosmic-app-list
    install -Dm0644 applets/input-sources/data/com.system76.CosmicAppletInputSources.desktop {{appdir}}/com.system76.CosmicAppletInputSources.desktop
    install -Dm0644 applets/workspaces/data/com.system76.CosmicAppletWorkspaces.desktop {{appdir}}/com.system76.CosmicAppletWorkspaces.desktop
    install -Dm0644 applets/app-tray/data/com.system76.CosmicAppList.desktop {{appdir}}/com.system76.CosmicAppList.desktop

uninstall:
    rm -f {{bindir}}/cosmic-applet-input-sources {{bindir}}/cosmic-applet-workspaces {{bindir}}/cosmic-app-list
    rm -f {{appdir}}/com.system76.CosmicAppletInputSources.desktop {{appdir}}/com.system76.CosmicAppletWorkspaces.desktop {{appdir}}/com.system76.CosmicAppList.desktop

test:
    cargo test --workspace

clean:
    cargo clean
