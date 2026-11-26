sample-rate-out-of-range = Une fréquence d’échantillonnage de { $rate } Hz est hors de la plage de { $lowest } à { $highest } Hz
channel-count-out-of-range = { $count ->
    [one] { $count } canal
   *[other] { $count } canaux
} est hors de la plage de 1 à { $most }
bit-depth-not-representable = { $bits ->
    [one] { $bits } bit
   *[other] { $bits } bits
} n’est pas une profondeur de bits représentable
decibels-not-finite = { $decibels } n’est pas un nombre fini de décibels
trim-out-of-range = Un ajustement de { $millibels ->
    [one] { $millibels } millième
   *[other] { $millibels } millièmes
} de décibel est hors de la plage de -{ $most } à { $most }
gain-not-finite = Un gain de { $gain } n’est pas un multiplicateur fini et positif ou nul
volume-out-of-range = Un volume de { $volume } est hors de la plage de 0 à 1
band-frequency-out-of-range = Une fréquence de bande de { $hertz } Hz est hors de la plage de { $lowest } à { $highest } Hz
band-gain-out-of-range = Un gain de bande de { $decibels } dB est hors de la plage de -{ $most } à { $most } dB
band-q-out-of-range = Un Q de { $q } est hors de la plage de { $least } à { $most }
preamp-out-of-range = Un préampli de { $decibels } dB est hors de la plage de -{ $most } à { $most } dB
speaker-angle-out-of-range = Un angle d’enceinte de { $degrees } ° est hors de la plage de { $least } à { $most } °
speaker-elevation-out-of-range = Une élévation d’enceinte de { $degrees } ° est hors de la plage de { $lowest } à { $highest } °
head-yaw-out-of-range = Une rotation de la tête de { $degrees } ° est hors de la plage de { $rightmost } à { $leftmost } °
zero-id = Un identifiant ne peut pas être zéro
source-name-not-usable = Une source se nomme avec des lettres minuscules, des chiffres, des tirets et des tirets bas
not-an-mbid = Ce n’est pas un identifiant MusicBrainz
not-an-isrc = Ce n’est pas un code ISRC
not-a-chromaprint = Ce n’est pas une empreinte Chromaprint écrite en base64 adaptée aux URL
year-not-held = Wrapped ne couvre pas l’année { NUMBER($year, useGrouping: "false") } ; choisissez-en une de { NUMBER($first, useGrouping: "false") } à { NUMBER($last, useGrouping: "false") }
