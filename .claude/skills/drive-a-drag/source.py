import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gio, GLib, Gtk

PATHS = sys.argv[1:]


def say(*words):
    print("source:", *words, flush=True)


def on_activate(app):
    window = Gtk.ApplicationWindow(application=app, title="dragsource")
    window.set_default_size(240, 140)
    label = Gtk.Label(label="drag me")
    window.set_child(label)

    source = Gtk.DragSource()
    source.set_actions(Gdk.DragAction.COPY)

    def prepare(_source, x, y):
        say("prepare", x, y)
        files = Gdk.FileList.new_from_list([Gio.File.new_for_path(path) for path in PATHS])
        return Gdk.ContentProvider.new_for_value(files)

    source.connect("prepare", prepare)
    source.connect("drag-begin", lambda *_: say("drag-begin"))
    source.connect("drag-end", lambda _s, _d, deleted: say("drag-end", deleted))
    source.connect("drag-cancel", lambda _s, _d, reason: say("drag-cancel", reason) or False)
    label.add_controller(source)

    click = Gtk.GestureClick()
    click.connect("pressed", lambda *_: say("pressed"))
    label.add_controller(click)

    window.present()
    say("shown")


app = Gtk.Application(application_id="org.resonate.dndsource")
app.connect("activate", on_activate)
app.run([])
