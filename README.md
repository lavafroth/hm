![A preview of setting render quality](/assets/preview.gif)

## `hm` 🤔 [![Rust Report Card](https://rust-reportcard.xuri.me/badge/github.com/lavafroth/hm)](https://rust-reportcard.xuri.me/report/github.com/lavafroth/hm)
A rusty TUI to auto-render Manim animations on save.


### Try it out

#### Via Cargo

```sh
cargo install --git https://github.com/lavafroth/hm.git
```

#### Via Nix (with flakes)

```
nix run github:lavafroth/hm
```

### Usage

Run `hm` in a project directory to start monitoring all Python files in it for changes.
If a Python file defining a Manim scene is modified, it will get re-rendered.

The keybinds in `hm` are inspired by [helix](https://helix-editor.com/) key chords. Press `space` to begin a chord, further keys pressed
either perform actions or enter their own context.

There's also a one line legend at the bottom of the screen to indicate what keys you should press next for respective actions.

#### Changing project directory

![A preview of setting render quality](/assets/project_directory.gif)

You may switch to a different project directory using `space`, `f` to enter the file picker. Use `hjkl` or the arrow keys to move
around. Once you are in the directory you wish to monitor, hit `space`.

#### Setting the render quality

![A preview of setting render quality](/assets/changing_quality.gif)

Begin a chord by pressing `space`, enter the quality settings context by pressing `q`. Now press one of the following keys for the
respective resolutions:

- `l`: 480p (default)
- `m`: 720p
- `h`: 1080p
- `p`: 1920p
- `k`: 4K

#### Triggering a re-render

![A preview of setting render quality](/assets/re_render.gif)

For some reason, if you want to re-render the last file. Press `space`, `r` to trigger a re-render.
