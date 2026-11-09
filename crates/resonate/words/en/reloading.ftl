taken-up = { $count ->
    [one] { $players } is running and takes this up
   *[other] { $players } are running and take this up
}
kept = { $count ->
    [one] { $players } is running and keeps the settings it started with; it hears this only if it reads this settings file, once it is started again
   *[other] { $players } are running and keep the settings they started with; they hear this only if they read this settings file, once they are started again
}
