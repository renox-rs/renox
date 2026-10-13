#!/usr/bin/env python3
"""Follows docs/tutorial.md the way a reader does, in an app made by `rnx new`.

Each step is found by the sentence that introduces it ("Replace `src/app/bookmarks/model.rs`
with:"), never by line number, and the `rnx` commands come from the tutorial's own bash
blocks. Code is taken as a reader sees it: rustdoc's hidden lines (`# …`) left out. When the
tutorial changes so that a step can no longer be followed, this script fails and says which
step: fix the tutorial (or this script, if the step changed on purpose).

    tests/tutorial/follow.py <app dir> <rnx binary> [renox checkout]

Run by tests/tutorial/run.sh, which then builds the app and runs its checks.
"""

import glob
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TUTORIAL = open(os.path.join(REPO, "docs/tutorial.md"), encoding="utf-8").read()


def fail(message):
    sys.exit(f"tutorial: {message}")


def block_after(anchor, nth=0):
    """The `nth` code block after the sentence `anchor` (whitespace may wrap)."""
    pattern = r"\s+".join(re.escape(word) for word in anchor.split())
    found = re.search(pattern, TUTORIAL)
    if not found:
        fail(f"no longer says: {anchor!r}")
    blocks = re.finditer(r"^```([^\n]*)\n(.*?)^```", TUTORIAL[found.end():], re.M | re.S)
    for i, block in enumerate(blocks):
        if i == nth:
            return shown(block.group(1), block.group(2))
    fail(f"no code block after {anchor!r}")


def shown(lang, body):
    """A block as rendered: rustdoc's hidden lines left out, `##` shown as `#`."""
    if not lang.startswith("rust"):
        return body
    lines = []
    for line in body.split("\n"):
        stripped = line.lstrip()
        if stripped.startswith("##"):
            lines.append(line.replace("##", "#", 1))
        elif stripped == "#" or stripped.startswith("# "):
            continue
        else:
            lines.append(line)
    return "\n".join(lines)


def read(path):
    return open(path, encoding="utf-8").read()


def write(path, text):
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    open(path, "w", encoding="utf-8").write(text)


def edit(path, old, new, step):
    text = read(path)
    if old not in text:
        fail(f"{step}: {path} has no {old!r} to change")
    write(path, text.replace(old, new, 1))


def run_rnx(block, rnx, step):
    """Runs the block's `rnx …` lines (not `rnx serve` and the like)."""
    for line in block.splitlines():
        line = line.split("#")[0].strip()
        if not line.startswith("rnx make:"):
            continue
        args = line.split()[1:]
        print(f"$ rnx {' '.join(args)}", flush=True)
        if subprocess.run([rnx, *args]).returncode != 0:
            fail(f"{step}: `rnx {' '.join(args)}` failed")


def module_lines(path):
    """The `pub mod …;` lines the generators added at the top of a module."""
    return [line for line in read(path).splitlines() if line.startswith("pub mod ")]


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    app, rnx = sys.argv[1], os.path.abspath(sys.argv[2])
    checkout = sys.argv[3] if len(sys.argv) > 3 else None

    # 1. Create the app.
    new = block_after("Then make the app and run it:")
    name = re.search(r"rnx new (\S+)", new).group(1)
    args = [rnx, "new", name] + (["--renox-path", checkout] if checkout else [])
    os.makedirs(app, exist_ok=True)
    if subprocess.run(args, cwd=app).returncode != 0:
        fail("1: `rnx new` failed")
    os.chdir(os.path.join(app, name))
    mod_rs = "src/app/bookmarks/mod.rs"
    index_html = "resources/views/bookmarks/index.html"

    # 2. A model and its table.
    run_rnx(block_after("then the model in it:"), rnx, "2")
    ups = glob.glob("migrations/*_create_bookmarks_table.up.sql")
    if len(ups) != 1:
        fail(f"2: expected one create_bookmarks_table migration, found {ups}")
    write(ups[0], block_after("Fill in `migrations/<timestamp>_create_bookmarks_table.up.sql`:"))
    write(ups[0].replace(".up.sql", ".down.sql"), block_after("and the `.down.sql` next to it"))
    write("src/app/bookmarks/model.rs", block_after("Replace `src/app/bookmarks/model.rs` with:"))

    # 3. A list page.
    write(mod_rs, "\n".join(module_lines(mod_rs)) + "\n" + block_after("below the `pub mod model;` line"))
    write(index_html, block_after("Replace `resources/views/bookmarks/index.html`:"))
    nav = block_after("links inside the `rx-navbar` tag, before the spacer:").splitlines()
    links = nav[1:nav.index(next(l for l in nav if "rx-spacer" in l))]
    layout = "resources/views/layouts/app.html"
    spacer = '    <span class="rx-spacer"></span>'
    edit(layout, spacer, "\n".join("  " + l for l in links) + "\n" + spacer, "3")

    # 4. A form that saves without a reload.
    edit(mod_rs, "use renox::prelude::*;\n", "use renox::Toast;\nuse renox::prelude::*;\n", "4")
    edit(mod_rs, "use model::Bookmark;\n", "use model::{Bookmark, BookmarkForm};\n", "4")
    write(mod_rs, read(mod_rs).rstrip("\n") + "\n\n" + block_after("Add this to `src/app/bookmarks/mod.rs`"))
    resource = next(l.strip() for l in block_after("And give the resource its `store` action:").splitlines()
                    if l.strip().startswith(".resource("))
    text = read(mod_rs)
    old = re.search(r"\.resource\(\"/bookmarks\"[^\n]*\)\)", text)
    if not old:
        fail("4: the module has no `.resource(\"/bookmarks\", …)` line to replace")
    write(mod_rs, text.replace(old.group(0), resource, 1))
    form = block_after("put the form between the `<rx-page-header>`")
    header = re.search(r"^ *<rx-page-header [^\n]*/>\n", read(index_html), re.M)
    if not header:
        fail("4: index.html has no <rx-page-header/> line to put the form under")
    edit(index_html, header.group(0),
         header.group(0) + "\n" + "".join("  " + l + "\n" if l else "\n" for l in form.rstrip("\n").split("\n")), "4")

    # 5. Edit and delete, only your own.
    run_rnx(block_after("may do what to a row:"), rnx, "5")
    write("src/app/bookmarks/policy.rs", block_after("Fill in `src/app/bookmarks/policy.rs`:"))
    write(mod_rs, "\n".join(module_lines(mod_rs)) + "\n" + block_after("Here is `src/app/bookmarks/mod.rs` with every action."))
    write("resources/views/bookmarks/edit.html", block_after("errors come back:"))
    row = block_after("give each row its buttons. Replace the `<li>`:").rstrip("\n").split("\n")
    text = read(index_html)
    li = re.search(r"^( *)<li [^\n]*>\n.*?^ *</li>", text, re.M | re.S)
    if not li:
        fail("5: index.html has no <li> to replace")
    indent = li.group(1)
    write(index_html, text.replace(li.group(0), "\n".join(indent + l if l else l for l in row), 1))
    auth = block_after("Send them to their bookmarks instead:")
    if ".redirect_to(\"/bookmarks\")" not in auth:
        fail("5: the login redirect step changed")
    edit("src/lib.rs", "Auth::new().account()", "Auth::new().account().redirect_to(\"/bookmarks\")", "5")

    # 6. A weekly digest mail.
    run_rnx(block_after("no Redis, no extra process, no cron entry."), rnx, "6")
    write("src/app/bookmarks/send_digest.rs", block_after("wrote `src/app/bookmarks/send_digest.rs`; replace it with:"))
    write("resources/views/mail/digest.html", block_after("Replace the HTML one:"))
    write("resources/views/mail/digest.txt", block_after("and the text one:"))
    schedule = block_after("Add the task to it, in `src/app/bookmarks/mod.rs`:").splitlines()
    uses = [l for l in schedule if l.startswith("use ")]
    job = next(i for i, l in enumerate(schedule) if "app.job::<" in l)
    end = next(i for i in range(job, len(schedule)) if schedule[i].strip() == "}")
    edit(mod_rs, schedule[job] + "\n", "\n".join(schedule[job:end]) + "\n", "6")
    edit(mod_rs, "use renox::prelude::*;\n", "\n".join(uses) + "\nuse renox::prelude::*;\n", "6")

    # 7. Tests and demo data.
    run_rnx(block_after("for tests and for a database to click around in:"), rnx, "7")
    write("src/app/bookmarks/bookmark_factory.rs", block_after("Replace `src/app/bookmarks/bookmark_factory.rs`:"))
    write("src/lib.rs", "mod app;\n\n" + block_after("with a seeder and the model exported for the tests"))
    write("tests/bookmarks.rs", block_after("Make `tests/bookmarks.rs`:"))
    if '.assert_redirect("/bookmarks")' not in TUTORIAL:
        fail("7: the tutorial no longer says how to change `guests_can_register`")
    edit("tests/home.rs", '.assert_redirect("/");', '.assert_redirect("/bookmarks");', "7")

    print(f"tutorial: followed every step in {os.getcwd()}")


if __name__ == "__main__":
    main()
