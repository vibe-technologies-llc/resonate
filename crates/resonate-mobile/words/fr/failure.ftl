no-device-waiting = Pas de haut-parleurs ou d’écouteurs à jouer — à jouer une fois qu’on est là
device-stopped-waiting = L’appareil audio a cessé de jouer — en jouant une fois qu’il y en a un
title-unreadable = Impossible de trouver -{ $title }- il peut avoir été déplacé ou supprimé
title-unsupported = Resonate ne peut pas lire le format de « { $title } »
title-damaged = Impossible de jouer à l’image de l’image -{ $title }- le fichier semble endommagé
title-cannot-seek = Impossible de changer la position de lecture dans « { $title } »
song-unopened = Cette chanson ne pouvait pas être ouverte.
song-cannot-seek = Cette chanson ne peut pas être ignorée.
nothing-playing = Rien ne joue en ce moment.
nothing-queued = Il n’y a rien dans la file d’attente à jouer
nothing-to-audition = Jouer une chanson d’abord : le test est entendu sur un jeu, et un flux DSD envoyé car il ne peut être commuté à l’intérieur
arms-alike = A et B sonneraient la même chose sur ce flux, donc il n’y a rien à dire à part
levels-unmeasured = A et B n’ont pas pu être mesurés l’un contre l’autre, de sorte que l’essai ne peut pas les niveler.
levels-too-far-apart = A et B jouent trop loin dans l’intensité pour être jumelés, de sorte que le test ne peut pas les nier
file-unreadable = Le fichier audio n’a pas pu être trouvé ou lu
file-unsupported = Le fichier audio est dans un format Resonate ne peut pas jouer
file-damaged = Le fichier audio semble endommagé
file-cannot-seek = Le fichier audio ne peut pas être ignoré
lookup-unreachable = Le service en ligne n’a pas pu être atteint pour { $asked }
lookup-offline = Les fonctionnalités en ligne sont désactivées, donc rien n’a été demandé au sujet { $asked }
lookup-metered = La connexion est mesurée, donc rien n’a été demandé sur { $asked }
lookup-refused = Le service en ligne a refusé la demande de { $asked } ({ $status })
lookup-unreadable = La réponse du service en ligne sur { $asked } ne pouvait pas être lue
lookup-too-large = La réponse du service en ligne sur { $asked } était trop grande pour lire
lookup-refused-in-a-row = Le service en ligne refusé { $refusals ->
    [one] { $refusals } demande
   *[other] { $refusals } demandes
} dans une rangée, la dernière pour { $asked }
lookup-failed = La recherche en ligne de { $asked } a échoué
measurement-too-large = Le fichier { $file } est plus grand que { $mebibytes } MIB
measurement-no-points = Aucune ligne du fichier { $file } ne se lit comme une fréquence et un niveau
measurement-too-few-points = Le fichier { $file } contient { $found ->
    [one] { $found } point de l’ordre du jour provisoire
   *[other] { $found } points
} et les besoins de réponse au moins { $least }
measurement-misses-the-mid-band = Le { $file } couvre { $lowest } Hz à { $highest } Hz et doit atteindre 500 Hz à 2 kHz
measurement-nothing-to-correct = La mesure se situe à l’intérieur { $off_by } dB de la cible, il n’y a donc rien à corriger.
measurement-unreadable = Le fichier { $file } n’a pas pu être lu
sweep-band = Un balayage jusqu’à 20 kHz nécessite un taux supérieur à 40 kHz et celui-ci court à { $rate }
sweep-heard-too-short = L’enregistrement tenu { $heard ->
    [one] { $heard } cadre
   *[other] { $heard } cadres
} et le balayage seul était { $played }, et il a été coupé court
sweep-heard-too-long = L’enregistrement du balayage était trop long pour mesurer
sweep-nothing-heard = Rien du balayage n’a été entendu : augmenter le volume ou rapprocher le microphone
sweep-lost-in-noise = Le balayage n’a pas pu être distingué du bruit ambiant : faire le silence dans la pièce, augmenter le volume ou rapprocher le microphone
sweep-unmeasured = Le balayage n’a pas pu être mesuré
no-device = Aucun haut-parleurs ou casque à jouer
device-gone = L’appareil audio a été débranché ou éteint
sound-server = Perte de contact avec le système sonore — réessayer
device-refused = L’appareil audio ne jouerait pas le format de cette chanson
queue-moved = La file d’attente a changé avant que cela ne se produise
player-stopped = Le lecteur a arrêté de répondre — redémarrer Resonate
unreadable = Une chanson n’a pas pu être trouvée
unsupported = Une chanson est dans un format Resonate ne peut pas jouer
damaged = Une chanson ne pouvait pas être jouée
held-by-another-zone = Une autre zone joue ce périphérique bit-perfect
zone-refused = Cette zone n’est plus là.
not-handed-over = L’autre zone n’a pas repris la lecture.
nothing-to-compare = On ne peut pas distinguer les deux côtés de l’examen d’écoute.
history-line = La ligne { NUMBER($line, useGrouping: "false") } du fichier n’est pas une écoute comme ListenBrainz l’écrit : { $fault ->
    [not-utf8] ce n’est pas un texte
    [not-json] ce n’est pas JSON
    [not-an-object] ce n’est pas un objet JSON
    [no-moment] il ne nomme pas le moment où il a été entendu à
   *[no-track-metadata] Il ne nomme pas de chanson
}
history-unkept = L’historique de l’écoute ne pouvait pas être lu ou écrit dans le catalogue de documents
