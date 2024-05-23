### `hm`

A rusty TUI to auto-render Manim animations on save.

### Installation

```sh
cargo install --git https://github.com/lavafroth/hm.git
```

### Usage

Run `hm` in the current working project directory. This will start monitoring all the Python files in the directory for changes.
If any modification is made to a file defining a Manim scene, it will get re-rendered.

The keybinds in `hm` are inspired by helix and acts on key chords. You can press `space` to start a chord, further keys pressed
either perform actions or enter their own context.

There's also a one line legend at the bottom of the screen to indicate what keys you should press next for respective actions.

#### Changing project directories

You may switch to a different project directory using `space`, `f` to enter the file picker. Use `hjkl` or the arrow keys to move
around. Once you are in the directory you wish to monitor, hit `space`.

#### Setting the render quality

Begin a chord by pressing `space`, enter the quality settings context by pressing `q`. Now press one of the following keys for the
respective resolutions:

- `l`: 480p (default)
- `m`: 720p
- `h`: 1080p
- `p`: 1920p
- `k`: 4K

#### Triggering a re-render

For some reason, if you want to re-render the last file. Press `space`, `r` to trigger a re-render.
