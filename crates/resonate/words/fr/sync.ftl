joins = Ce dispositif rejoint les dispositifs de { $code }
left = Ce périphérique a laissé ses périphériques ; un nouveau code d’appariement est fait lors de la prochaine synchronisation
forgotten = { $device } est hors de la liste de vos appareils
pairing-code = Code d’appariement
server = Serveur
no-server = aucun, les appareils sur ce réseau seulement
standing = En position debout
on-this-network = sur ce réseau
online = en ligne
away = loin d’ici
this-device = { $where }, ce dispositif
lacks-nothing = { $playlist } manque de rien { $from } pourrait envoyer
came-from = { $landed } des { $missing ->
    [one] { $missing } chanson
   *[other] { $missing } chansons
} vient de { $from }
fetch-already-held = { $count ->
    [one] { $count } chanson était déjà ici
   *[other] { $count } chansons étaient déjà ici
}
fetch-claimed = { $count ->
    [one] { $count } chanson était déjà en cours de récupération par un autre transfert
   *[other] { $count } chansons étaient déjà en cours de récupération par un autre transfert
}
fetch-unreachable = { $count ->
    [one] { $from } n’a pas pu être joint pour { $count } chanson
   *[other] { $from } n’a pas pu être joint pour { $count } chansons
}
fetch-failed = { $count ->
    [one] { $count } chanson n’est pas arrivée
   *[other] { $count } chansons ne sont pas arrivées
}
fetch-unanswered = { $count ->
    [one] { $count } chanson est restée sans réponse à temps
   *[other] { $count } chansons sont restées sans réponse à temps
}
zone = Zone
state = État
playing = Jouer
volume = Volume
percent = { $percent }%
queue = File d’attente
songs = { $songs ->
    [one] { $songs } chanson
   *[other] { $songs } chansons
}
billed = { $title } — { $artist }
idle = inactif
playing-now = jouer
paused = pause
stopped = arrêté
connecting = connexion
connected = connecté
unreachable = le serveur n’a pas pu être atteint
refused = le serveur a refusé ce périphérique
nearby = synchronisation avec les appareils de ce réseau
