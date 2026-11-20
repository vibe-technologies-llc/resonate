removed = La copie de MusicBrainz est retirée.
none-was-held = Aucune copie de MusicBrainz n’a été conservée.
built = La copie de MusicBrainz est construite à { $path } ({ $gigabytes } GB) en { $minutes ->
    [one] { $minutes } minute
   *[other] { $minutes } minutes
}.
mirror-off = musicbrainz-mirror est éteint, donc les recherches ne le demandent pas jusqu’à ce qu’il soit allumé.
none-held = Aucune copie de MusicBrainz n’est conservée. `resonate musicbrainz --build` en fait une.
found = { $title } par { $credited }
held = L’exportation de { $dump } est tenue à { $path } ({ $gigabytes } GB).
held-though-off = L’exportation de { $dump } est tenue à { $path } ({ $gigabytes } GB), bien que musicbrainz-mirror soit éteint.
answers-kept = { $answers ->
    [one] { $answers } la réponse de recherche est maintenue à partager avec les appareils synchronisés.
   *[other] { $answers } les réponses de recherche sont gardées à partager avec les appareils synchronisés.
}
replicated = La copie de MusicBrainz suit les modifications jusqu’au paquet { $to } ({ $changes ->
    [one] { $changes } changement
   *[other] { $changes } changements
}, { $documents ->
    [one] { $documents } entrée réécrite
   *[other] { $documents } entrées réécrites
}).
already-replicated = La copie de MusicBrainz contient déjà tous les changements publiés, jusqu’au paquet { $to }.
amended = Il suit les changements de MusicBrainz jusqu’au paquet { $sequence }, appliqué { $ago }.
