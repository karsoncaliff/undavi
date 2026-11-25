# Migrating

How to move to a newer version of Undavi by hand.

From 0.10.0 on, Undavi tells you about new releases and can install them for you.
0.9.0 can't, so moving on from it takes the steps below, once.

## From 0.9.0 to 0.10.0

### 1. Back up your settings

Quit Undavi, then copy your settings somewhere safe:

```sh
cp ~/.config/undavi/settings.json ~/undavi-settings-0.9.0.json
```

Your settings move over by themselves, so this is only for
[going back](#going-back-to-090): 0.9.0 can't read the settings 0.10.0 saves.

### 2. Install 0.10.0

There's no need to uninstall 0.9.0 first: 0.10.0 replaces it. Install it the same way
you installed 0.9.0. If you aren't sure which way that was, run `command -v undavi`:

| It says | You installed |
|---|---|
| `/usr/bin/undavi` | The [Arch Linux](#arch-linux) package |
| `/usr/local/bin/undavi` | The [download](#download), or [from source](#from-source) |

The install script moved from `packaging/install.sh` to `install.sh`. If you
installed 0.9.0 with a `PREFIX` of your own, use the same one again:
`sudo PREFIX=/your/prefix ./install.sh`.

#### Download

Get `undavi-0.10.0-x86_64-linux.tar.gz` from the
[releases](https://github.com/karsoncaliff/undavi/releases), then:

```sh
tar xf undavi-0.10.0-x86_64-linux.tar.gz
cd undavi-0.10.0-x86_64-linux
sudo ./install.sh
```

#### Arch Linux

In the folder you cloned Undavi into (or a new clone):

```sh
git pull
cd packaging/arch
makepkg -si
```

#### From source

In the folder you cloned Undavi into (or a new clone):

```sh
git pull
cargo build --release --locked
sudo ./install.sh
```

### 3. Check that it worked

Start Undavi and click the logo. The About panel should say **Version 0.10.0**.

If it still says 0.9.0, quit it and start it again. If that doesn't help, there may
be a second copy: `type -a undavi` lists them all. Remove the one you don't want
with the script or package manager that installed it.

### Your settings and presets

Your settings carry over, and each visualizer and background starts from the ones you
had. Presets are now saved per section, so each of your old ones is split up and keeps
its name:

| A preset from | Becomes a preset in |
|---|---|
| Tuning | Sound |
| Visuals | Visualizer and Colors |
| Appearance | Background, Colors, Now playing, Lyrics and General |

Colors, Now playing, Lyrics and General only get one if the preset changed something
there, and identical ones are kept once. Colors holds both the palette and the accent
color, which used to be in different presets, so the one a preset didn't have comes
from your current settings.

### Where things moved

One settings window replaces the three side panels. Open it with the gear at the top
right, or with S.

| In 0.9.0 | In 0.10.0 |
|---|---|
| Tuning (T) | Sound (T) |
| Visuals (V) | Visualizer (V) |
| Visuals → Color | Colors |
| Appearance (A) → Background | Background (B) |
| Appearance → Background → Dim | Background → Effects |
| Appearance → Now playing | Now playing |
| Appearance → Lyrics | Lyrics |
| Appearance → Accent | Colors |
| Appearance → Interface | General (I) |
| Background motion **Spiral** | Background motion **Rotate** |

The A shortcut is gone. Spiral is now the name of a new animated background effect.

### Updates from here on

When it starts, 0.10.0 looks for a newer release on GitHub. What happens next depends
on how you installed it:

- **Download or from source:** it offers to install the update, asking for your
  password if it needs to. Tick **Update automatically** to skip the question next time.
- **Arch Linux:** it only tells you. Follow the [Arch Linux](#arch-linux) steps again.

This works on 64-bit PCs (x86-64). To turn it off, choose **Don't ask again** in the
pop-up, or turn off **Check for updates** in the settings, under General.

### Going back to 0.9.0

Quit Undavi and install 0.9.0 again, as its README describes. With the download,
that's:

```sh
tar xf undavi-0.9.0-x86_64-linux.tar.gz
cd undavi-0.9.0-x86_64-linux
sudo packaging/install.sh
```

Then, before starting it, put your old settings back:

```sh
cp ~/undavi-settings-0.9.0.json ~/.config/undavi/settings.json
```

This replaces the settings from 0.10.0, so copy those first if you want to keep them.
Without this step, 0.9.0 starts with the default settings and no presets, and keeps a
copy of the ones from 0.10.0 as `~/.config/undavi/settings.json.v4`.
