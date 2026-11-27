format-of = { $codec } { $rate } kHz, { $bits } bits
candidates = { $count ->
    [one] Un fichier FLAC occupe { $bytes }.
   *[other] { $count } fichiers FLAC occupent { $bytes }.
} Rien n’est écrit sans --apply.
shrunk = { $file } : de { $before } à { $after }
kept = { $file } : gardé tel quel, { $why }
failed = { $file } : gardé tel quel, la copie n’a pas pu être faite : { $error }
saved = { $count ->
    [one] Un fichier réécrit plus petit
   *[other] { $count } fichiers réécrits plus petits
}, { $saved } gagnés.
not-held = { $file } n’est pas un fichier de la bibliothèque.
nothing-held = La bibliothèque ne contient aucun fichier FLAC entier à recompresser.
nothing-stored = La bibliothèque ne contient aucun fichier.
why-no-smaller = car il n’est pas plus petit une fois réécrit
why-unverified = car la copie ne se relit pas telle qu’elle a été écrite
why-changed-meanwhile = car le fichier a changé pendant sa réécriture
why-halted = car il a été arrêté avant la fin
why-not-a-flac = car ce n’est pas un fichier FLAC
why-vaulted = car sa chanson est gardée dans l’archive
why-cut-by-sheet = car une feuille cue le découpe en chansons
why-outside-the-library = car il n’est sous aucun dossier de la bibliothèque
why-head-unread = car son en-tête FLAC n’a pas pu être lu
why-id3-after-the-audio = car il porte une étiquette ID3v1 après le son
why-ape-after-the-audio = car il porte une étiquette APE après le son
why-beyond-the-encoder = car son son en { $rate } kHz, { $bits } bits dépasse ce que l’encodeur écrit, sauf si --to nomme un plafond
why-tied-to-the-samples = car son bloc { $block } désigne des endroits du son qu’un nouveau débit ou une nouvelle profondeur déplacerait
why-disagrees-with-its-digest = car son son ne correspond pas à la somme de contrôle qu’il porte
why-wider-than-its-depth = car ses échantillons dépassent la profondeur qu’il déclare
why-turns-partway = car son son change de forme en cours de route
why-undecodable = car son son n’a pas pu être décodé
why-silent = car il ne contient aucun son
why-held-under-other-names = car il est lié sous d’autres noms, qui garderaient l’ancienne copie
why-attributes-not-carried = car la copie n’a pas pu recevoir le propriétaire, les droits ou les attributs du fichier
