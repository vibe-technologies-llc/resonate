store = Le catalogue de documents ne pouvait pas être { $op } : { $source }
no-provider = Aucun fournisseur de téléchargement n’est configuré
not-found = Aucun fournisseur n’avait cette chanson
unreachable = { $provider } n’a pu être atteint
provider-away = { $provider } n’a pas répondu à temps
provider-refused = { $provider } a rejeté la demande ({ $status })
account-refused = { $provider } n’a pas accepté le compte (erreur { NUMBER($code, useGrouping: "false") })
provider-untrusted = { $provider } a un certificat que ce téléphone ne fait pas confiance
provider-misaddressed = { $provider } n’est pas à cette adresse
provider-unreadable = { $provider } a répondu avec quelque chose qui ne pouvait pas être lu
sign-in-lapsed = La signature { $provider } a expiré avant son approbation.
sign-in-denied = Le signe { $provider } a été refusé.
download-io = Le téléchargement ne pouvait pas être { $op }
download-cancelled = Le téléchargement a été annulé
download-expired = L’offre de téléchargement a expiré; rechercher à nouveau
unknown-download = Aucun téléchargement n’est sous ce numéro
download-too-large = Le fichier téléchargé dépasse la limite des huit GiB
output-unavailable = cette sortie audio n’est pas disponible
renderers-unreachable = Renders sur le réseau ne peut pas être joué à partir de ce téléphone
renderer-search = Le réseau n’a pas pu être recherché
renderer-address = Cette adresse n’est pas un rendu peut être atteint à
receiver-search = Le réseau n’a pas pu être consulté pour les récepteurs Cast
receiver-address = Il ne s’agit pas d’une adresse ou d’un nom qu’un récepteur Cast peut atteindre à
receiver-unreached = Ce récepteur Cast n’a pas pu être atteint
catalog-not-offered = Le catalogue ne pouvait pas être offert sur le réseau
unknown-renderer = Ce rendeur n’est plus sur le réseau
renderer-in-use = Ce rendu est en train de jouer ; arrêtez-le avant d’essayer ce qu’il joue
renderer-unlearnt = Ce rendeur n’a pas pu être jugé
receiver-unlearnt = Ce récepteur Cast ne pouvait pas être essayé
invalid-descriptor = le descripteur de destination est invalide
wrong-song = Le fichier téléchargé n’est pas la chanson demandée: sa longueur diffère
equaliser = le profil égalisateur n’a pas pu être lu
unknown-target = Aucune cible intégrée n’est appelée { $name }
unknown-account = ce service ne reconnaît pas le compte
analysis = l’analyse audio a échoué
lyrics = les paroles ne pouvaient pas être lues
lyrics-too-large = Les paroles sont plus grandes qu’un MiB ({ $bytes ->
    [one] { $bytes } octet
   *[other] { $bytes } octets
})
clip-too-long = Le clip dure plus de trente secondes ({ $samples ->
    [one] { $samples } échantillon
   *[other] { $samples } échantillons
})
no-local-matches = aucun de ces enregistrements n’a pu être associé à la bibliothèque locale.
not-a-playlist-link = Ce n’est pas un lien vers une playlist sur Spotify, Deezer, Apple Music, TIDAL, YouTube ou ListenBrainz
no-playlist-link = Cette playlist n’a pas été importée d’un lien
not-a-profile-link = Ce n’est pas un lien vers un profil Deezer
no-profile-at-the-link = Ce profil est privé, ou ne montre aucune playlist
not-signed-in-to-spotify = Aucun compte Spotify n’est signé
no-spotify-client = Donnez d’abord l’identifiant du client de votre application Spotify
spotify-sign-in-lapsed = Spotify ne connaît plus ce compte : reconnectez-vous
spotify-sign-in = L’inscription Spotify n’a pas fini : { $failure }
unreadable-playlist-link = Le lien de cette liste de lecture ne peut plus être lu
playlist-file-too-large = Le fichier de liste de lecture est plus grand que huit MIB ({ $bytes ->
    [one] { $bytes } octet
   *[other] { $bytes } octets
})
playlist-file-empty = Le fichier playlist ne contient aucune chanson
playlist-file-unwritten = La playlist n’a pas pu être écrite dans un fichier
invalid-wanted-song = un enregistrement sauvegardé doit avoir un titre
online-off = Les fonctionnalités en ligne sont désactivées. Allumez-les dans Paramètres.
lookups-held-by-the-meter = La connexion est mesurée, donc rien n’a été regardé vers le haut. Autoriser les recherches sur une connexion mesurée dans Paramètres.
no-served-server = Aucun service de jeu n’est mis en place
heard = Le clip n’a pas pu être nommé : { $source }
invalid-playlist-name = un nom de playlist doit avoir entre un et 255 octets
duplicate-playlist-name = une autre playlist a déjà ce nom
unknown-playlist = la playlist n’existe plus
playlist-changed = la playlist a changé; rafraîchir avant l’édition
playlist-followed = cette playlist est suivie, donc seules les personnes qui l’éditent peuvent la modifier
not-a-list = Une recherche sauvegardée se remplit; ses chansons ne peuvent pas être ajoutées, supprimées ou déplacées
not-a-saved-search = Une playlist de chansons n’a aucune recherche à réviser
invalid-picture = l’image n’est pas soutenue ou trop grande
invalid-uri = le document URI n’est pas pris en charge
catalog-version = document catalogue version { $found } est plus récent que ce build supporte
unknown-track = la piste demandée n’est pas dans la bibliothèque
too-many-bands = Un égaliseur détient au plus des bandes { $most } et non { $found }
band-frequency = La bande { $band } doit s’asseoir entre { $lowest } Hz et { $highest } kHz, et non { $hertz } Hz
band-gain = La bande { $band } peut booster ou couper au plus { $most } dB, et non { $decibels } dB
band-q = La Q de la bande { $band } doit être comprise entre { $least } et { $most }, et non entre { $q }
band-channels = Bande { $band } est jouée sur aucun canal
preamp-range = Le préampli peut soulever ou descendre au maximum { $most } dB, et non { $decibels } dB
listening-test-trials = Un test d’écoute tient de 1 à { $most } essais, et non { $trials }
listening-test-nothing-to-test = La courbe n’a pas de bandes et pas de préampli, donc il n’y a rien à comparer avec son contournement
listening-test-levels = La courbe ne peut être appariée au niveau de sa dérivation, { $millibels ->
    [one] { $millibels } millième
   *[other] { $millibels } millièmes
} d’un décibel séparé, de sorte qu’il n’est pas testé
listening-test-finished = Ce test d’écoute n’a plus de procès à répondre.
buffer-range = longueur du tampon { $milliseconds } ms est à l’extérieur de 50 à 2000 ms
closed = le lecteur a été fermé
no-zone = Il n’y a pas de zone { $zone }
zone-unnamed = Une zone doit avoir un nom
first-zone-stays = La première zone reste
unknown-queue-row = aucune ligne de queue n’a l’identificateur { $queue_id }
queue-row-out-of-range = file d’attente { $row } est en dehors d’une file d’attente de { $rows }
sync = Synchronisation du périphérique ne pouvait pas { $op }
sync-off = La synchronisation du périphérique est désactivée. Allumez-le dans les périphériques.
unknown-sync-device = aucun périphérique en synchronisation n’est appelé que
nothing-handed-off = que l’appareil ne joue rien tenu ici
unknown-received-song = que la chanson d’un autre appareil n’attend plus
received = La chanson d’un autre appareil ne pouvait pas être { $op }
musicbrainz-copy-not-removed = La copie de MusicBrainz n’a pas pu être retirée : { $source }
sync-certificate = Ce fichier n’est pas un certificat auquel cet appareil peut faire confiance : { $source }
sync-certificate-file = Le certificat du serveur ne pouvait pas être { $op }
layout-unreadable = La mise en page ne peut pas être lue : { $source }
tags-unwritable = Les étiquettes de ce type de fichier ne peuvent pas être écrites
tags-unconfirmed = Le fichier n’a pas été lu avec les balises écrites
tags-document = Le fichier de la chanson ne pouvait pas être { $op }
tags-unrestored = Le fichier de la chanson a été laissé endommagé par une écriture ratée et n’a pas pu être remis; son original est conservé à { $kept }
tag-number = Un numéro de piste ou de disque doit être un nombre entier supérieur à zéro
no-lyrics-kept = Pas de paroles pour cette chanson.
lyrics-cannot-stand-beside = Le dossier de cette chanson a été délivré ne permet pas qu’une feuille lyrique soit écrite à côté
lyrics-unsaved = Les paroles ne peuvent pas être sauvegardées à côté de la chanson
lyrics-document = La feuille lyrique à côté de la chanson ne pouvait être { $op }
lyrics-document-unread = La feuille lyrique à côté de la chanson ne pouvait pas être lue
sheet-read = lire
sheet-made = fabriqués
sheet-opened = ouvert
sheet-written = écrit
impulse-extension = Un fichier de correction de salle doit être un fichier audio
impulse-too-large = Le fichier de correction de chambre est plus grand qu’un téléphone garde
impulse-not-staged = Aucun fichier de correction de salle n’est mis en place
no-impulse = Le fichier ne contient aucune correction de salle de réponse peut utiliser
impulse-pair = Les réponses des deux intervenants ne sont pas une paire : { $source }
impulse-file = Le fichier de correction de salle ne pouvait pas être { $op }
folder = le dossier d’application n’a pas pu être créé
store-created = créé
store-opened = ouvert
store-laid-out = établi
store-read = lire
store-written = écrit
download-opened = ouvert
download-read = lire
download-written = écrit
download-finished = fini
rewrite-copied-aside = copié de côté pour modifier
rewrite-opened = ouvert à l’écriture
rewrite-written = écrit
rewrite-read-back = lire en arrière
rewrite-put-back = remise
sync-start = début
sync-mint = créer un nouveau code d’appariement
sync-missing = liste les chansons manquantes de cet appareil
sync-keep = gardez vos changements
sync-handoffs = lire ce que d’autres appareils jouent
sync-forget = oublier ce dispositif
received-opened = ouvert
received-copied = copié
received-finished = fini
impulse-kept = conservé
impulse-found = trouvé
impulse-removed = enlevé
quick-connect-off = Ce serveur a Quick Connect désactivé, alors connectez-vous avec le mot de passe
sign-in-cancelled = La signature a été annulée.
history-file = Le fichier d’historique d’écoute ne pouvait pas être { $op }
history-file-read = lire
history-file-written = écrit
no-history-user = Nommer le { $service ->
    [lastfm] Last.fm
   *[listenbrainz] ListenBrainz
} utilisateur dont l’écoute pour apporter, ou enregistrer un compte pour lui d’abord
no-lastfm-application = Donnez d’abord la clé API et le secret partagé de votre app Last.fm
lossy-for-a-replacement = Le fichier téléchargé est perdu, et une copie sans perte a été demandée comme une version de meilleure qualité
not-a-variant = Cette copie n’est pas conservée comme une version de cette chanson

explore-already-held = Cette chanson est déjà dans votre bibliothèque.
