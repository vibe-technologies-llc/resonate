const wanted = {
    "dragsource": {x: 40, y: 40, width: 240, height: 140},
    "Resonate": {x: 320, y: 60, width: 1200, height: 760}
};
for (const w of workspace.windowList()) {
    print("window", w.caption, w.frameGeometry.x, w.frameGeometry.y, w.frameGeometry.width, w.frameGeometry.height);
    const place = wanted[w.caption];
    if (place) {
        w.frameGeometry = place;
        print("placed", w.caption);
    }
}
