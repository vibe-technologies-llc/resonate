started-from = { $name }: a commencé à { $service }, { $kept }
read-from = { $name }: lire { $service }, { $kept }
followed-on = { $name }: suivi { $service }
moved-once-on = { $name }: déplacé à Resonate une fois, et n’a plus suivi { $service }
let-go = { $playlists ->
    [one] { $playlists } playlist laissé aller
   *[other] { $playlists } Listes de lecture abandonnées
}
deleted-there = { $playlists ->
    [one] { $playlists } playlist supprimé là
   *[other] { $playlists } playlists supprimées là
}
unread = { $playlists ->
    [one] { $playlists } playlist non lue
   *[other] { $playlists } playlists non lues
}
none-in-step = aucune liste de lecture n’est conservée en pas avec un lien; `resonate import <LINK> --upkeep in-step` en garde une
spotify-not-signed-in = le compte Spotify est suivi mais n’est pas signé; `resonate spotify` signes
its-link = son lien
was-already = { $name } était déjà { $kept }
in-step-now = { $name } est maintenu à l’étape { $link } maintenant
left-as-it-stands = { $name } est laissé tel quel, et { $link } n’est pas lu de nouveau seul
no-longer-follows = { $name } ne suit plus un lien; ses lignes restent
came-from-no-link = { $name } vient d’aucun lien
came-from = { $service }, { $upkeep }
once = une fois
in-step = dans l’étape
names-no-playlist = { $name }: { $service } n’appelle plus aucune liste de lecture; ses rangs restent tels qu’ils étaient
read-again = { $name }: lire à nouveau du { $service }, { $moved }
imported-once = importé une fois
kept-in-step = tenue à l’étape
nothing-to-add = rien à ajouter
added = ajouté { $songs ->
    [one] { $songs } chanson
   *[other] { $songs } chansons
}
took-out = sortie { $songs ->
    [one] { $songs } chanson
   *[other] { $songs } chansons
}
added-and-took-out = ajouté { $added ->
    [one] { $added } chanson
   *[other] { $added } chansons
} et sortit { $removed ->
    [one] { $removed } chanson
   *[other] { $removed } chansons
}
wanted = recherché { $songs ->
    [one] { $songs } chanson
   *[other] { $songs } chansons
} la bibliothèque manque, qui `resonate poll` récupère
not-wanted = { $songs ->
    [one] { $songs } chanson
   *[other] { $songs } chansons
} ne pouvait pas être voulu
unnamed = { $songs ->
    [one] { $songs } chanson
   *[other] { $songs } chansons
} Nommé rien MusicBrainz sait
profile-not-read-again = { $page }: ne peut être relire à partir de { $service }: { $error }
playlist-not-read-again = { $name }: ne peut être relire à partir de { $service }: { $error }
