cover-art = couvrir l’art
taken-away = (tiré)
rating = Note
unknown = inconnu
none = aucune
the-network = le réseau
a-device = un dispositif
the-graph = le graphique
nothing-plugged-in = { $port } (rien n’est branché)
plugged-in = { $port } (branché)
volume-in-software = { $port }, volume dans le logiciel
volume-on-the-hardware = { $port }, volume sur le matériel
volume-always-on-the-device = { $port }, volume toujours sur l’appareil
heard-at = { $port } à { $level }
muted = { $port }, muté
scanned = découvert { $discovered }- ajouté { $added }- mis à jour { $updated }- déplacé { $moved }- enlevé { $removed }- échoué { $failed }
cancelled = annulé
carrying-on = poursuivre la recherche une course gauche inachevée
stopped-unreached = stoppé : la référence n’a pas pu être atteinte en demandant { $asked }
stopped-refused-in-a-row = arrêté: la référence continue de refuser, la dernière fois demandant { $asked }
stopped-offline = stoppé : la connexion en ligne est désactivée
stopped-metered = stoppé : la connexion est mesurée, et l’enrichissement sur mesure est désactivé
nothing-asked = rien n’a été demandé
asked-release = une libération
asked-find-release = une recherche de libération
asked-recording = un enregistrement
asked-isrc = un enregistrement par son ISRC
asked-find-recording = une recherche d’enregistrement
asked-release-group = un groupe de libération
asked-find-release-group = une recherche de groupe de libération
asked-artist = un artiste
asked-find-artist = une recherche d’artiste
asked-release-groups-of-artist = les libérations d’un artiste
asked-releases-of-group = les pressures d’un relâchement
asked-releases-of-artist = les pressages d’un artiste
asked-cover = une couverture
asked-portrait = un portrait
asked-lyrics = paroles
asked-devices = les dispositifs mesurés
asked-correction = a correction mesurée
asked-recognise = une reconnaissance
asked-submit = une soumission de ce qui a été entendu
asked-love = un favori raconté à un service d’écoute
asked-token = une vérification du jeton d’un service d’écoute
asked-history = une page de ce qu’un service d’écoute a entendu
asked-stream-link = un chercher où une piste coule
asked-follow-link = quels noms de liens
asked-share-file = un téléchargement de fichier public
asked-latest-release = la dernière sortie de Resonate
asked-spotify-account = une connexion au compte Spotify
nothing-wanted = rien n’est recherché
held = tenue
gave-up = J’ai abandonné après { $tries ->
    [one] { $tries } Essayez
   *[other] { $tries } tente
}
again-in = { $standing }, de nouveau dans { $left }
due-again = { $standing }, attendu
offered = offert
due = exigibles
unanswered = sans réponse
no-match = pas de correspondance { $misses } de { $tries }
lasting-minutes = { $minutes }m
lasting-hours = { $hours }h
lasting-hours-and-minutes = { $hours }h { $minutes }m
read-the-rest = lire { $releases ->
    [one] { $releases } libération
   *[other] { $releases } rejets
} plus de la discographie de { $artist }
dismissed-beside = { $dismissed } rejeté, --bring-back les énumère de nouveau
brought-back = ramenés { $tracks ->
    [one] { $tracks } voie rejetée
   *[other] { $tracks } Voies abandonnées
} et { $releases ->
    [one] { $releases } non libéré
   *[other] { $releases } rejets rejetés
}
nothing-missing = Il ne manque rien.
nothing-of-missing = rien de { $artist } n’est manquant
missing-across = { $tracks ->
    [one] { $tracks } piste
   *[other] { $tracks } voies
} manquants de l’autre côté { $albums ->
    [one] { $albums } album
   *[other] { $albums } albums
} · { $releases ->
    [one] { $releases } libération
   *[other] { $releases } rejets
} non détenu
tracks-missing = DÉPLACEMENTS
every-track-held = chaque piste de sortie que le catalogue connaît est tenue
releases-not-held = LIBÉRATIONS NON TENUES
every-release-held = chaque libération d’un artiste détenu est tenue
credits-unread = MusicBrainz crédits { $artist } { $groups ->
    [one] { $groups } groupe de libération
   *[other] { $groups } groupes de libération
} plus qui n’étaient pas lus
yes = oui
no-provider = aucun fournisseur n’est enregistré
polled = demandé { $asked }-Offre { $offered }-Conservée { $kept }-Conservée { $unkept }-Rien { $nothing }-Ré refusée { $refused }-Délai { $late }
forgot = Oublié { $root }
forgot-delivered = oublié la livraison { $path }
forgot-gone = Oublié { $tracks ->
    [one] { $tracks } piste
   *[other] { $tracks } voies
} Parti de { $folder }
forgot-nothing = { $path } n’était ni une racine de bibliothèque, ni une piste livrée, ni un dossier tenant une piste disparue
copies = { $copy } exemplaires { $kept }
differs = { $path } est numéroté comme une copie mais diffère, donc il reste
to-fold = { $copies ->
    [one] { $copies } copie
   *[other] { $copies } exemplaires
} à plier; --apply les replie
folded = { $copies ->
    [one] { $copies } copie
   *[other] { $copies } exemplaires
} plié et envoyé à la poubelle
trash-refused = { $kept ->
    [one] { $kept } la poubelle ne prendrait pas, donc elle reste
   *[other] { $kept } la poubelle ne prendrait pas, donc ils restent
}
changed-since-found = { $path } a changé depuis qu’il a été trouvé, il reste donc
left-beside = { $path } reste où il était, la poubelle ne le prendrait pas
plays = { $plays ->
    [one] { $plays } jouer
   *[other] { $plays } pièces
}
favourite-plays = favori · { $plays }
not-favourite-plays = pas un favori · { $plays }
nothing-to-tag = rien à marquer
lyric-lines = { $count ->
    [one] une ligne de paroles
   *[other] { $count } lignes de paroles
}
sheet-track = Voie { $number } { $field }
written = Écrit { $written }-Champs { $fields }-Photos { $pictures }-Avis { $ratings }
would-write = would write { $written }-champs { $fields }-photos { $pictures }-notes { $ratings }
tagged = { $written }-- déjà dit { $unchanged }-- passé { $passed }-- marchait { $walked }
nothing-written = rien n’a été écrit; passez --apply pour le faire
nothing-to-put-back = aucun essai appliqué n’est retenu pour remettre en état
nothing-to-organise = rien à organiser
moved = mouvementé { $moved }
folders-pruned = dossiers taillés { $folders }
would-move = se déplacerait { $moved }
folders-to-empty = dossiers à vider { $folders }
organised = { $moved } , en place { $unchanged } , non identifié { $unidentified } , entré en collision { $collided } , a échoué { $failed } , { $folders }
nothing-moved = rien n’a été déplacé; passez --apply pour le faire
refused-with = { $standing }: { $why } { $with }
refused-kind = { $standing }: { $why } ({ $kind })
refused = { $standing }: { $why }
unidentified = rien ne nomme un album pour le déposer sous
loose = il débarquerait dans la racine
collided = Il serait en conflit avec
shares-a-sheet = il serait séparé d’un autre fichier nommé par
named-from-above = il est coupé par une feuille dans un dossier au-dessus,
out-of-reach = son chemin serait plus long que Windows ne prend
source-gone = ce n’est plus là où le catalogue dit
unmoved = le volume a refusé le déplacement
explain-source = Source: { $spec }
explain-dsd = { $rate } ({ $hertz } Hz, 1-bit)
explain-sink = sortie audio: { $description } ({ $name })
explain-output = sortie: { $stream } [{ $mode }]
explain-words = mots: offerts comme { $offered }; les noms de l’appareil { $named }
explain-remix = Remix: { $from } -> { $to }
explain-resample = rééchantillonnage : { $from } → { $to } [{ $quality }, phase { $phase }]
explain-eq = eq:      { $bands ->
    [one] { $bands } bande
   *[other] { $bands } bandes
}, pic { $peak } dB, préampli { $preamp }
explain-eq-unbound = eq: on, mais rien de ce dispositif n’est lié à façonner le son
explain-room = chambre: { $taps } touche un canal sur { $channels } canaux, mesuré à { $rate }
found-as-it-plays = trouvé comme il joue
studied-at = étudié à { $kilohertz } kHz
explain-restore = restaurer: { $restoration } une source { $tuning }, son mur { $wall }
explain-crossfeed = aliments croisés: { $crossfeed }
explain-guard = garde: véritables pics maintenus sous -0.1 dBTP
explain-dither = Dither: à { $depth } [{ $shaping }]
explain-dop = Dop: { $reading }
quality-low-latency = faible latence
quality-balanced = équilibrée
quality-high = élevée
quality-very-high = très élevée
phase-linear = linéaire
phase-intermediate = intermédiaire
phase-minimum = minimale
tuning-mp3 = MP3
tuning-aac = AAC
tuning-vorbis = Vorbis
shaping-flat = plat
shaping-lipshitz = lipshitz
shaping-threshold = seuil
above-nyquist = { $bands ->
    [one] { $bands } bande est au-dessus du Nyquist de la sortie et n’est pas appliquée
   *[other] { $bands } bandes sont au-dessus du Nyquist de la sortie et ne sont pas appliquées
}
dop-packed = emballés sur PCM, intacts
dop-off = décimé à PCM; la dop est éteinte
dop-inexact = décimé à PCM; la sortie audio ne prend pas exactement le transporteur
dop-gain = décimée en PCM; un stade de gain réécrirait les marqueurs
a-list-kept-in = une liste conservée en { $order }
a-list = une liste
itself-with = lui-même, { $text }
itself = lui-même
in-order = { $order } ordre
in-reverse-order = ordre inverse { $order }
by-album = album
by-artist = artiste
by-title = titre
by-length = longueur
by-file-name = nom du fichier
renamed = { $name } s’appelle { $into } maintenant
exported = écrit { $rows } lignes de { $name } à { $path } comme { $format }
back-in-hand = { $name } est de retour dans la main, et une rangée ajoutée à elle atterrit à la fin
holds-nothing = { $name } ne tient rien à jouer
copied = copie { $copied } de { $name } en { $target }, qui détient maintenant { $held }
query-revised = { $name } se remplit maintenant{ $matching }, et détient { $held } maintenant
query-saved = { $name } se remplit{ $matching }, et détient { $held } maintenant
reads-as = qu’il se lit comme { $reads }
whatever-matches = avec tout ce qui correspond { $text }
the-whole-library = avec toute la bibliothèque
added = ajouté { $added } à { $name }, qui détient maintenant { $held }
imported = { $name }: lire { $format } comme { $encoding }, ajouté { $added }, qui détient maintenant { $held }
already-in-order = { $name } était déjà dans cet ordre
reordered = { $moved ->
    [one] déplacée { $moved } ligne de { $name } dans cet ordre
   *[other] { $moved } lignes de { $name } ont été réordonnées
}
kept-already = { $name } est conservé en { $order }, et était déjà en lui
now-kept = { $moved ->
    [one] { $name } est conservé dans { $order }, et { $moved } rangée déplacée dans elle
   *[other] { $name } est conservé dans { $order }, et { $moved } rangées y sont déplacées
}
all-there = chaque ligne de { $name } nomme toujours un fichier qui est là
tidied = { $dropped ->
    [one] largué { $dropped } ligne de { $name } dont le fichier est parti
   *[other] largué { $dropped } lignes de { $name } dont les fichiers sont partis
}
none-doubled = chaque ligne de { $name } nomme un fichier qu’aucune autre ligne ne fait
folded-doubles = { $dropped ->
    [one] pliée { $dropped } rangée doublée de { $name } dans celle ci-dessus
   *[other] plié { $dropped } rangées doubles de { $name } dans celles ci-dessus
}
pinned = { $name } est épinglé, et se trouve en haut de la liste
unpinned = { $name } n’est pas épinglé, et prend sa place dans l’ordre où la liste se lit dans
discarded-search = { $name } et la recherche qu’elle s’est remplie de
discarded-empty = { $name }, qui ne tenait rien
discarded = { $held ->
    [one] { $name } rejeté et la ligne { $held } qu’il détenait; le fichier reste là où il est
   *[other] { $name } et les lignes { $held } qu’il tenait; les fichiers restent là où ils sont
}
none-match = rien en { $name } matches { $text }, donc aucune ligne n’a été retirée
took-out = { $dropped ->
    [one] a pris { $dropped } ligne correspondant { $text } sur { $name }
   *[other] a pris { $dropped } lignes correspondant à { $text } sur { $name }
}
already-in-it = { $already ->
    [one] { $already } était déjà dedans
   *[other] { $already } étaient déjà dedans
}
passed-over = transmis { $passed } n’appelant aucun fichier local
fell-short = { $short ->
    [one] la feuille déclarée { $short } ligne qu’elle ne tenait pas
   *[other] la feuille déclarée { $short } lignes qu’elle ne tenait pas
}
not-on-disk = { $missing ->
    [one] { $missing } nomme un fichier qui n’est pas là
   *[other] { $missing } fichiers de noms qui ne sont pas là
}
queued-onto = en attente { $queued } sur { $player }
moving-to = déplacer ce que { $player } joue dans la zone { $zone }
no-player = aucun joueur de cette construction ne fonctionne
nothing-queued = rien n’est en attente sur { $player }
now-playing-playlist = { $player } joue { $playlist }
in-the-background = jouer en arrière-plan, donc aucune clé n’est lue; fg et une interruption l’arrêtent
unknown-key = clé inconnue { $key }; ? pour l’aide
started = commencé { $track }
finished = terminée { $track }
output = sortie { $stream } [{ $mode }] sur sortie audio { $sink }
underrun = cadres inférieurs { $missing }
failed = { $track } échoué
waits = { $track } attend un appareil
rejected = { $command } rejeté
untuned = la fenêtre déjà ouverte n’a pas pris les paramètres donnés pour cette exécution
handed = à la fenêtre s’ouvre déjà comme { $window }
output-bit-perfect = fidèle au bit
output-repacked = reconditionné
output-dithered = tramé
output-converted = converti
