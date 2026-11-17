# speci-tui

The terminal edition of speci-local: the specification editor, the test cases, the evaluation
(results, step-by-step stepper) and the structogram editor, **all driven from the keyboard** with
vim-style keys. It uses the same engine as the window app, so `.sp` / `.st` files and copied
project lines are interchangeable between the two.

```sh
cargo run --release -p speci-tui              # a sample specification
cargo run --release -p speci-tui -- task.sp   # or open a file (.sp or .st)
cargo build --release -p speci-tui            # → target/release/speci-tui
```

It builds without any of the windowing libraries (no GTK, no xkbcommon): only the engine and
`ratatui`/`crossterm`. Press `?` (or `F1`) inside for the full key list. The help is context-aware: general keys come
first, then the sections for the tool and pane you are in (marked "you are here"), and the
`Space` hint line shows the shortcuts that apply to the current tool.

## Layout and sizes

Designed for **150×40** and up, usable far below that:

| terminal | specification tool |
|---|---|
| ≥ 120 columns | three panes side by side: specification · test data · evaluation |
| 70–119 columns | specification on the left, test data over evaluation on the right |
| smaller | one pane at a time (its tab strip is in the title) |

`Ctrl-w o` zooms the focused pane to the whole screen at any size. Every pane scrolls
(`j/k`, `Ctrl-d/u`, `gg/G`), long lines wrap, and the structogram canvas scrolls in both directions.
The mouse is never needed (and never captured, so you can still select text with it).

## Keys at a glance

| | |
|---|---|
| `Tab` / `Shift-Tab`, `Ctrl-w h/l/w/1/2/3` | move between panes |
| `Ctrl-w o` | zoom / unzoom the pane |
| `gt` `gT` `F2` `Ctrl-w t` | Specification ⇄ Structogram |
| `]t` `[t` | next / previous test case |
| `]e` `[e` | next / previous error line |
| `Space r` (`F5`) | evaluate now |
| `Space w` (`Ctrl-s`), `W` | save, save as |
| `Space o` `n` `q` | open, new, quit |
| `Space y` / `p` | copy / paste the whole project as one line |
| `Space t` `d` `s` | new test case, delete test case, insert variable names |
| `Space f` (`:fromspec`) | **convert the specification into structograms** |
| `Space a` / `T` | auto-evaluation / light-dark colors |
| `:` | command line: `:w :wq :q :q! :e :run :tnew :tdel :t N :set light` … |

**Text editors** (specification and test data) speak vim: operators, motions, text objects,
counts, `.`, registers, visual modes, `u`/`Ctrl-r`, `*` `#`, `/` search with `n` `N`. In insert
mode `Ctrl-n`/`Ctrl-p` complete headers, `MACROS`, keywords and words from the text (and `forall`,
`exists`, `leq`, `geq`, `neq` turn into symbols), `Ctrl-k` starts a digraph
(`Ctrl-k (-` → ∈, `FA` → ∀, `TE` → ∃, `!=` → ≠, `=<` → ≤, `>=` → ≥), `Alt-1…6` types ∈ ∀ ∃ ≠ ≤ ≥,
and typing `<=`, `>=`, `!=` converts them as in the window app.

**Evaluation pane**: results with the explanation of every problem (`Enter` jumps to it in the
editor), the stepper (`h/l` step through the evaluation, the evaluated part is highlighted) and a
step list. `v` cycles the views.

**Structogram tool**: the diagrams are drawn as Nassi–Shneiderman boxes. `j/k` walk the elements,
`h` goes to the parent, `l` inside, `w/b` between siblings, `{`/`}` between structograms.
`e`/`i` edit the text of the element (`Enter` saves, `Esc` cancels), `o`/`O` add an element after/before
(then `s` statement, `d` decision, `w` while loop, `r` repeat loop, `c` multi-way), `dd`/`yy`/`p`/`P`
delete/copy/paste, `J`/`K` move an element, `+c`/`+d` add a case / default, `th` `tv` `tr` show or
hide the name, variables and return value, `u`/`Ctrl-r` undo and redo. The test cases run in the
background; the failing element is marked red and the side panel says why.

## ASCII mode

For terminals or fonts without good Unicode (the Linux console, serial lines, odd SSH setups):

```sh
speci-tui --ascii          # or SPECI_ASCII=1; switched on automatically when the locale isn't UTF-8
```

or `:set ascii` / `Space A` while running (`:set utf8` / `Space A` again to go back; `--utf8`
forces Unicode). Every character on the screen is then 7-bit ASCII: borders are `+ - |` (the focused
pane's are `# =`), diagrams are plain `+---+` boxes, `[ok]`/`[FAIL]` replace the check marks, and
accented letters are shown without their accents (`Paratlan szamok`).

The conversion is done inside the renderer, on the finished frame before the terminal is updated,
so scrolling and partial redraws stay correct. An external "UTF-8 to ASCII" filter on the output
can't do that, because the program only sends the cells that changed.

**Symbols.** The language accepts words for its symbols, so in ASCII mode they are *displayed* as the
word you can type: `n∈N` shows as `n in N`, `∀` as `forall`, `∃` as `exists`, `≤ ≥ ≠` as `<= >= !=`,
`¬` as `not`. The file on disk keeps the real symbols, so it stays compatible with the window app.

**Typing symbols without Alt** (works in every mode, in the specification and in structogram text):

| type | result | | type | result |
|---|---|---|---|---|
| `\in` | ∈ | | `\le` or `<=` | ≤ |
| `\all` | ∀ | | `\ge` or `>=` | ≥ |
| `\ex` | ∃ | | `\ne` or `!=` | ≠ |
| `\not` | ¬ | | `\nd` | ∤ |

The name converts when you type a space or punctuation, press Tab, or leave insert mode. `Ctrl-k`
digraphs (`Ctrl-k (-` gives ∈) and, where the terminal passes it on, `Alt-1…6` work too. You never
actually need the symbols: `in`, `forall`, `exists`, `not`, `<=`, `>=`, `!=` are valid as typed.

## Clipboard

There is no clipboard library. Copy uses `wl-copy`, `xclip`, `xsel`, `pbcopy` or `clip.exe` when
present and always sends the OSC 52 escape too, so copying works over SSH and in tmux
(`set -g set-clipboard on`) in terminals that allow it. Paste with your terminal's paste shortcut:
the text arrives as a paste event and goes into whatever you are editing; a pasted project line
outside insert mode is imported.

## Differences from the window app

* Structogram **colors** (box background, text color) and the PNG export are not in the terminal
  version. `Space c` / `:export file.txt` give the picture as plain text instead; the file format
  keeps colors made in the window app untouched.
* No automatic session restore: open the file again.
* The spec/data text editors are always modal (vim).

## Development

```sh
cargo fmt -p speci-tui
cargo clippy -p speci-tui --all-targets   # pedantic + nursery, see the lint block in src/main.rs
cargo test -p speci-tui                   # drives the app with scripted keys on an in-memory terminal
```

`src/vim/` is a copy of the window app's vim engine (`src/editor/vim`) without the egui glue;
`many_single_char_names` is allowed crate-wide for it and for the x/y/w/h geometry code.
