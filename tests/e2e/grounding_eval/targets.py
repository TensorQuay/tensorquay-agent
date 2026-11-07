"""The grounding evaluation's target set (docs/evals/computer-grounding.md): 200 targets on synthetic pages and one
desktop application, each an instruction in plain words and the element it means, named by the role and name the
`computer` tool's snapshot gives it. The boxes are not here: the capture takes each one from the page's own DOM (or
the application's AT-SPI tree) on the Desktop image, so they are the product's own ground truth.

Deterministic: the same seed gives the same pages, targets and order. `python3 targets.py OUT` writes `pages/*.html`
and `targets.json` for the capture. `--canvas` adds the 60 canvas targets after the 200 (canvas_targets.py), their
pages beside the others and "canvas": {"seed", "count"} in the file; without it the file is the 200's, as before.
"""

import argparse
import html
import json
import random
from pathlib import Path

import canvas_targets

SEED = 20261006
# How many targets of each kind; they sum to TOTAL.
KINDS = {"form": 40, "menu": 30, "icon": 30, "table": 30, "chinese": 25, "dark": 25, "desktop": 20}
TOTAL = 200
# The Desktop's screen; a page must fit the browser's view at this size without scrolling.
SCREEN = (1280, 800)

FORM_FIELDS = [
    ("textbox", "Email", "email"), ("textbox", "Full name", "text"), ("textbox", "Phone", "tel"),
    ("textbox", "Company", "text"), ("textbox", "City", "text"), ("textbox", "Postcode", "text"),
    ("textbox", "Password", "password"), ("textbox", "Website", "url"), ("searchbox", "Search orders", "search"),
    ("spinbutton", "Quantity", "number"),
]
FORM_CHOICES = [
    ("checkbox", "Subscribe to the newsletter"), ("checkbox", "I accept the terms"), ("checkbox", "Remember me"),
    ("radio", "Monthly billing"), ("radio", "Yearly billing"), ("combobox", "Country"), ("combobox", "Language"),
]
FORM_BUTTONS = ["Save", "Cancel", "Submit order", "Reset", "Back", "Continue"]
MENU_BAR = ["File", "Edit", "View", "Insert", "Format", "Tools", "Help"]
MENU_ITEMS = {
    "File": ["New", "Open", "Save as", "Export", "Print", "Close"],
    "Edit": ["Undo", "Redo", "Cut", "Copy", "Paste", "Find and replace"],
    "View": ["Zoom in", "Zoom out", "Full screen", "Show ruler", "Show grid", "Outline"],
    "Tools": ["Spelling", "Word count", "Macros", "Options", "Plugins", "Translate"],
}
# Small icon-only buttons: what the icon shows, its accessible name, and the glyph drawn.
ICONS = [
    ("a gear", "Settings", "⚙"), ("a trash can", "Delete", "\U0001F5D1"), ("a pencil", "Edit", "✎"),
    ("a magnifying glass", "Search", "\U0001F50D"), ("a star", "Add to favourites", "★"),
    ("a bell", "Notifications", "\U0001F514"), ("a house", "Home", "⌂"), ("a printer", "Print", "⎙"),
    ("a lock", "Lock", "\U0001F512"), ("a cross", "Close", "✕"), ("a plus sign", "Add", "✚"),
    ("a download arrow", "Download", "⤓"), ("a share arrow", "Share", "➦"), ("a heart", "Like", "♥"),
    ("a refresh arrow", "Refresh", "↻"), ("a question mark", "Help", "?"), ("an envelope", "Mail", "✉"),
    ("a calendar", "Calendar", "\U0001F4C5"), ("a camera", "Take a photo", "\U0001F4F7"), ("a flag", "Report", "⚑"),
]
CHINESE_FIELDS = [("textbox", "姓名"), ("textbox", "电子邮件"), ("textbox", "手机号码"), ("textbox", "公司名称"),
                  ("textbox", "详细地址")]
CHINESE_BUTTONS = ["保存", "取消", "提交订单", "下一步", "返回首页", "删除", "导出报表", "立即购买"]
CHINESE_LINKS = ["帮助中心", "隐私政策", "联系我们", "常见问题"]
# Mousepad (GTK 3) as AT-SPI names it: its menu bar, and its File menu once open. The capture matches a name whole,
# after GTK's mnemonic, ellipsis and padding ("Open...      "); an item that opens a submenu is a menu, not a menuitem
# (Open Recent, New From Template), as the first capture found (12 Oct 2026).
SUBMENUS = {"Open Recent", "New From Template"}
DESKTOP = {
    "mousepad-main": [("menu", m) for m in ["File", "Edit", "Search", "View", "Document", "Help"]],
    "mousepad-file": [("menu" if m in SUBMENUS else "menuitem", m)
                      for m in ["New", "New Window", "Open", "Open Recent", "Reload", "Save", "Save As", "Save All",
                                "Print", "Close Tab", "Close Window", "Quit", "Detach Tab", "New From Template"]],
}


def page(title, body, dark=False, lang="en"):
    colours = ("#111418", "#e6e6e6", "#2a2f36") if dark else ("#ffffff", "#1a1a1a", "#f0f0f0")
    style = (f"body{{margin:24px;font:14px sans-serif;background:{colours[0]};color:{colours[1]}}}"
             f"input,select,button{{font:inherit;background:{colours[2]};color:{colours[1]};border:1px solid #888}}"
             "label{display:inline-block;margin:6px 18px 6px 0}table{border-collapse:collapse}"
             "td,th{padding:2px 8px;border:1px solid #999}.icon{width:24px;height:24px;padding:0;font-size:14px}")
    return (f"<!doctype html><html lang={lang}><meta charset=utf-8><title>{html.escape(title)}</title>"
            f"<style>{style}</style><body>{body}</body></html>")


def target(kind, page_id, number, instruction, role, name):
    return {"id": f"{page_id}-{number:02d}", "kind": kind, "page": page_id, "instruction": instruction,
            "role": role, "name": name}


def form_pages(rng, count, kind="form", dark=False):
    pages, targets = {}, []
    per_page = 10
    for index in range(count // per_page + (1 if count % per_page else 0)):
        page_id = f"{kind}-{index + 1:02d}"
        fields = rng.sample(FORM_FIELDS, 5)
        choices = rng.sample(FORM_CHOICES, 3)
        buttons = rng.sample(FORM_BUTTONS, 2)
        parts, chosen = [], []
        for role, name, kind_of_input in fields:
            parts.append(f"<label>{html.escape(name)} <input type={kind_of_input} aria-label='{html.escape(name)}'"
                         f" style='width:{rng.randint(120, 260)}px'></label>")
            chosen.append((role, name, f"Click the {name} field."))
            if rng.random() < 0.3:
                parts.append("<br>")
        for role, name in choices:
            if role == "combobox":
                parts.append(f"<label>{html.escape(name)} <select aria-label='{html.escape(name)}'><option>One"
                             "<option>Two</select></label>")
                chosen.append((role, name, f"Open the {name} list."))
            else:
                parts.append(f"<label><input type={role} name=g aria-label='{html.escape(name)}'>"
                             f"{html.escape(name)}</label>")
                chosen.append((role, name, f"Tick “{name}”."))
        parts.append(f"<div style='margin-top:{rng.randint(10, 300)}px;margin-left:{rng.randint(0, 700)}px'>")
        for name in buttons:
            parts.append(f"<button style='margin-right:{rng.randint(4, 40)}px'>{html.escape(name)}</button>")
            chosen.append(("button", name, f"Press the {name} button."))
        parts.append("</div>")
        pages[page_id] = page(f"Form {index + 1}", "".join(parts), dark=dark)
        for number, (role, name, instruction) in enumerate(chosen[:per_page], start=1):
            targets.append(target(kind, page_id, number, instruction, role, name))
    return pages, targets[:count]


def menu_pages(rng, count):
    pages, targets = {}, []
    for index, open_menu in enumerate(["File", "Edit", "View", "Tools"][: (count + 9) // 10]):
        page_id = f"menu-{index + 1:02d}"
        bar = "".join(f"<span role=menuitem tabindex=0 style='padding:4px 12px'>{name}</span>" for name in MENU_BAR)
        items = "".join(f"<div role=menuitem tabindex=-1 style='padding:4px 24px'>{name}</div>"
                        for name in MENU_ITEMS[open_menu])
        left = 12 + 64 * MENU_BAR.index(open_menu)
        body = (f"<div role=menubar style='background:#ddd;padding:4px'>{bar}</div>"
                f"<div role=menu aria-label='{open_menu}' style='position:absolute;left:{left}px;top:52px;"
                f"background:#fff;border:1px solid #888;box-shadow:2px 2px 6px #888'>{items}</div>"
                f"<p style='margin-top:{rng.randint(260, 500)}px'>Document body.</p>")
        pages[page_id] = page(f"Menus {index + 1}", body)
        chosen = [(name, f"In the open {open_menu} menu, choose {name}.") for name in MENU_ITEMS[open_menu]]
        chosen += [(name, f"Click {name} in the menu bar.") for name in rng.sample(
            [name for name in MENU_BAR if name != open_menu], 4)]
        for number, (name, instruction) in enumerate(chosen, start=1):
            targets.append(target("menu", page_id, number, instruction, "menuitem", name))
    return pages, targets[:count]


def icon_pages(rng, count):
    pages, targets = {}, []
    for index in range((count + 9) // 10):
        page_id = f"icon-{index + 1:02d}"
        icons = rng.sample(ICONS, 10)
        parts = []
        for row in range(2):
            top = rng.randint(20, 520) if row else rng.randint(0, 40)
            parts.append(f"<div style='margin-top:{top}px;margin-left:{rng.randint(0, 900)}px'>")
            for description, name, glyph in icons[row * 5:(row + 1) * 5]:
                parts.append(f"<button class=icon aria-label='{html.escape(name)}' title='{html.escape(name)}'"
                             f" style='margin-right:{rng.randint(2, 12)}px'>{glyph}</button>")
            parts.append("</div>")
        pages[page_id] = page(f"Toolbar {index + 1}", "".join(parts))
        for number, (description, name, _) in enumerate(icons, start=1):
            targets.append(target("icon", page_id, number, f"Click the icon that shows {description}.", "button",
                                  name))
    return pages, targets[:count]


def table_pages(rng, count):
    pages, targets = {}, []
    for index in range((count + 9) // 10):
        page_id = f"table-{index + 1:02d}"
        rows = []
        invoices = [f"INV-{rng.randint(1000, 9999)}" for _ in range(18)]
        invoices = list(dict.fromkeys(invoices))
        for invoice in invoices:
            amount = f"{rng.randint(10, 9999)}.{rng.randint(0, 99):02d}"
            rows.append(f"<tr><td>{invoice}</td><td>{amount}</td><td>{rng.choice(['Paid', 'Due', 'Late'])}</td>"
                        f"<td><a href='#' aria-label='Open {invoice}'>Open</a></td>"
                        f"<td><button aria-label='Edit {invoice}'>Edit</button></td></tr>")
        body = ("<table><tr><th>Invoice</th><th>Amount</th><th>State</th><th></th><th></th></tr>"
                + "".join(rows) + "</table>")
        pages[page_id] = page(f"Invoices {index + 1}", body)
        picked = rng.sample(invoices, 10)
        for number, invoice in enumerate(picked, start=1):
            if number % 2:
                targets.append(target("table", page_id, number, f"Press Edit in the row for invoice {invoice}.",
                                      "button", f"Edit {invoice}"))
            else:
                targets.append(target("table", page_id, number, f"Click Open in the row for invoice {invoice}.",
                                      "link", f"Open {invoice}"))
    return pages, targets[:count]


def chinese_pages(rng, count):
    pages, targets = {}, []
    for index in range((count + 8) // 9):
        page_id = f"chinese-{index + 1:02d}"
        fields = rng.sample(CHINESE_FIELDS, 3)
        buttons = rng.sample(CHINESE_BUTTONS, 4)
        links = rng.sample(CHINESE_LINKS, 2)
        parts = [f"<label>{name} <input aria-label='{name}' style='width:{rng.randint(140, 260)}px'></label>"
                 for _, name in fields]
        parts.append(f"<div style='margin:{rng.randint(20, 300)}px 0 0 {rng.randint(0, 800)}px'>")
        parts += [f"<button style='margin-right:{rng.randint(4, 30)}px'>{name}</button>" for name in buttons]
        parts.append("</div><p>")
        parts += [f"<a href='#' style='margin-right:20px'>{name}</a>" for name in links]
        pages[page_id] = page(f"中文页面 {index + 1}", "".join(parts) + "</p>", lang="zh")
        chosen = [("textbox", name, f"点击“{name}”输入框。") for _, name in fields]
        chosen += [("button", name, f"点击“{name}”按钮。") for name in buttons]
        chosen += [("link", name, f"点击“{name}”链接。") for name in links]
        for number, (role, name, instruction) in enumerate(chosen, start=1):
            targets.append(target("chinese", page_id, number, instruction, role, name))
    return pages, targets[:count]


def desktop_targets(count):
    targets = []
    for page_id, items in DESKTOP.items():
        for number, (role, name) in enumerate(items, start=1):
            where = "in Mousepad's menu bar" if page_id == "mousepad-main" else "in Mousepad's open File menu"
            targets.append(target("desktop", page_id, number, f"Click {name} {where}.", role, name))
    return targets[:count]


def build(seed=SEED):
    """The pages (id to HTML) and the targets, in order."""
    rng = random.Random(seed)
    pages, targets = {}, []
    for builder, kind in ((form_pages, "form"), (menu_pages, "menu"), (icon_pages, "icon"),
                          (table_pages, "table"), (chinese_pages, "chinese")):
        made, chosen = builder(rng, KINDS[kind])
        pages.update(made)
        targets += chosen
    made, chosen = form_pages(rng, KINDS["dark"], kind="dark", dark=True)
    pages.update(made)
    targets += chosen
    targets += desktop_targets(KINDS["desktop"])
    return pages, targets


def check(pages, targets):
    """The set's own rules: the counts, unique ids, and each page's targets unique by role and name."""
    assert len(targets) == TOTAL, len(targets)
    for kind, count in KINDS.items():
        assert sum(1 for item in targets if item["kind"] == kind) == count, kind
    assert len({item["id"] for item in targets}) == TOTAL
    seen = set()
    for item in targets:
        key = (item["page"], item["role"], item["name"])
        assert key not in seen, key
        seen.add(key)
        assert item["kind"] == "desktop" or item["page"] in pages, item["page"]


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("out", nargs="?", default=".")
    parser.add_argument("--canvas", action="store_true", help="add the 60 canvas targets after the 200")
    args = parser.parse_args(argv)
    pages, targets = build()
    check(pages, targets)
    out = Path(args.out)
    (out / "pages").mkdir(parents=True, exist_ok=True)
    for page_id, text in pages.items():
        (out / "pages" / f"{page_id}.html").write_text(text, encoding="utf-8")
    desktop = sorted({item["page"] for item in targets if item["kind"] == "desktop"})
    written = {"seed": SEED, "screen": SCREEN, "desktop": desktop}
    if args.canvas:
        files, painted = canvas_targets.build()
        for name, text in files.items():
            (out / "pages" / name).write_text(text, encoding="utf-8")
        written["canvas"] = {"seed": canvas_targets.SEED, "count": len(painted)}
        targets = targets + painted
    (out / "targets.json").write_text(json.dumps({**written, "targets": targets}, ensure_ascii=False, indent=1),
                                      encoding="utf-8")


if __name__ == "__main__":
    main()
