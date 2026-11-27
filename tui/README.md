# speci-tui

The terminal edition of speci-local: the specification editor, the test cases, the evaluation
(results, step-by-step stepper), the structogram editor and the **C# code tool** (write the
program from the specification and the structograms, run it with dotnet), **all driven from the keyboard** with
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
| `gt` `gT` `F2` `Ctrl-w t` | Specification → Structogram → Code → … |
| `]t` `[t` | next / previous test case |
| `]e` `[e` | next / previous error line |
| `Space r` (`F5`) | evaluate now |
| `Space w` (`Ctrl-s`), `W` | save, save as |
| `Space o` `n` `q` | open, new, quit |
| `Space y` / `p` | copy / paste the whole project as one line |
| `Space u` | **links and conversions menu** (see below; `:url :tourl :toline :decode :csurl :csline :weblink :openweb`) |
| `Space s` (Code tool, `:fromcode`) | **convert the C# program into structograms** |
| `Space t` `d` `s` | new test case, delete test case, insert variable names |
| `Space f` (`:fromspec`) | **convert the specification into structograms** (in the Code tool: into a C# program) |
| `Space m` `r` `x` `i` `b` | Code tool: mode test/console, run, stop, fill input, carbon link |
| `Space a` / `T` | auto-evaluation / light-dark colors |
| `:` | command line: `:w :wq :q :q! :e :run :tnew :tdel :t N :set light` … |

**Text editors** (specification and test data) speak vim: operators, motions, text objects,
counts, `.`, registers, visual modes, `u`/`Ctrl-r`, `*` `#`, `/` search with `n` `N`. In insert
mode `Ctrl-n`/`Ctrl-p` complete headers, `MACROS`, keywords and words from the text (and `forall`,
`exists`, `leq`, `geq`, `neq` turn into symbols), `Ctrl-k` starts a digraph
(`Ctrl-k (-` → ∈, `FA` → ∀, `TE` → ∃, `!=` → ≠, `=<` → ≤, `>=` → ≥), `Alt-1…6` types ∈ ∀ ∃ ≠ ≤ ≥,
and typing `<=`, `>=`, `!=` converts them as in the window app.

**Code tool** (`Space 3`, `:code`): one C# editor (vim keys, highlighting, braces-aware indent).
`Space f` writes the program from the specification and the structograms (the ones of the
Structogram tool, or what *From spec* would build when it is empty); a program you edited is never
replaced without asking. `Space m` chooses whether the program contains the selected test case's
values (`test values`, it prints `name = value`) or reads its input from the console (the input
box, `Space i` fills it from the test case) and prints plain values. `Space r` / `F5` builds
and runs it with the `dotnet` on the `PATH` (Linux, macOS, Windows), `Space x` stops it; compile
errors mark their lines (`Enter` in the output jumps to the first, `]e` / `[e` walk them). In
the console mode the **expected output** of the test case is shown under the input and, after a
run, the output is compared with it. `Space b` copies a carbon.now.sh link that shows the program
as an image (`B` opens it in the browser). The program is saved in the `.sp` file together with
the specification, the tests and the structograms.

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

## Links and conversions

The ELTE web editors (progalap.elte.hu) share projects as links with `?data=` in them; this
program shares them as `SPECI1:` / `STUKI1:` lines. `Space u` opens a menu that converts between
the forms; the input is the clipboard (or the command's argument, or for the C# conversions the
program of the Code tool) and the result is copied. Wrapped lines are fine.

| key | command | conversion |
|---|---|---|
| `a` | `:url [text]` | guess: a line becomes a link, a link becomes a line |
| `l` | `:tourl [text]` | line, specification text or `.st` JSON → web editor link |
| `u` | `:toline [link]` | web editor link → `SPECI1:` / `STUKI1:` line |
| `d` | `:decode [link] [file]` | web editor link → specification text or structogram JSON (also into a file) |
| `c` | `:csurl` | C# program → Stuki link, one structogram per method |
| `C` | `:csline` | C# program → `STUKI1:` line |
| `w` | `:weblink` | copy the web editor link of this tool's project (Code tool: the program as structograms) |
| `o` | `:openweb` | open it in the browser |

Without a clipboard tool the menu waits for the terminal's paste. **C# → structograms**
(`Space s` or `:fromcode` in the Code tool) turns every method of the program into a structogram
and opens the Structogram tool; the result replaces its content. It is a scanner for textbook
algorithms (loops, `if`, `switch`, declarations, `Console` input and output), not a C# compiler:
what it does not recognize stays as a statement line.

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
cargo clippy -p speci-tui --all-targets   # workspace lints + pedantic + nursery, see the block in src/main.rs
cargo test -p speci-tui                   # drives the app with scripted keys on an in-memory terminal
```

`src/vim/` is a copy of the window app's vim engine (`src/editor/vim`) without the egui glue;
`many_single_char_names` is allowed crate-wide for it and for the x/y/w/h geometry code.
