just-now = just now
now = now
years-ago = { $count ->
    [one] { $count } year ago
   *[other] { $count } years ago
}
months-ago = { $count ->
    [one] { $count } month ago
   *[other] { $count } months ago
}
weeks-ago = { $count ->
    [one] { $count } week ago
   *[other] { $count } weeks ago
}
days-ago = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
}
hours-ago = { $count ->
    [one] { $count } hour ago
   *[other] { $count } hours ago
}
minutes-ago = { $count ->
    [one] { $count } minute ago
   *[other] { $count } minutes ago
}
years-briefly = { $count }y
months-briefly = { $count }mo
weeks-briefly = { $count }w
days-briefly = { $count }d
hours-briefly = { $count }h
minutes-briefly = { $count }m
