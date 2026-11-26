this-devices-own-curve = La courbe de cet appareil
every-other-devices-own-curve = La courbe de chaque autre appareil
own-curve-file = courbe propre.txt
measurement-lines-passed-over = { $lines ->
    [one] ; { $lines } ligne de la mesure a été dépassée
   *[other] ; { $lines } lignes de la mesure ont été dépassées
}
fitted = En attente { $name }, { $bands ->
    [one] { $bands } bande
   *[other] { $bands } bandes
} montés vers { $aimed_at }: { $before } dB avant et { $after } dB après
measured = mesurée
measurement-stopped = La mesure a été arrêtée; rien n’a été conservé.
nothing-heard = Rien n’a été entendu — vérifiez que le microphone est allumé et que la sortie peut être entendue
no-output-for-the-sweep = La sortie pour jouer le balayage n’est pas là
sweep-lost-in-noise = Le microphone a entendu la pièce mais aucun balayage au-dessus de son bruit — faites le silence dans la pièce, augmentez le volume ou rapprochez le microphone
sweep-unheard = Je ne pouvais pas jouer le balayage et l’entendre
fit-that-measurement = correspondent à cette mesure
lines-passed-over = { $count ->
    [one] ; { $count } ligne cette construction n’a pas été lue a été passée
   *[other] ; { $count } lignes que cette construction ne lit pas ont été passées
}
rolloffs-approximated = { $count ->
    [one] ; { $count } le filtre nomme un rolloff il se lit comme deuxième ordre
   *[other] ; { $count } filtres nom un rolloff il se lit comme deuxième ordre
}
values-clamped = { $count ->
    [one] ; { $count } valeur passée ce qu’il détient a été retenue à ses limites
   *[other] ; { $count } valeurs passées ce qu’il détient ont été tenues à ses limites
}
bound-but-not-kept = { $curve } est lié mais n’est pas gardé ici, il joue donc à plat
read-that-profile = lire ce profil
save-the-curve = Enregistrer la courbe
carry-the-curve = porter la courbe à l’appareil renommé
bands-at-most = une courbe maintient au maximum les bandes { $most }
kept-graphic = a maintenu une courbe graphique comme { $name }, { $bands ->
    [one] { $bands } bande
   *[other] { $bands } bandes
} ici et encore à la vitesse que chaque flux joue à
kept-with-bands = En attente { $name }, { $bands ->
    [one] { $bands } bande
   *[other] { $bands } bandes
}
import-that-profile = importer ce profil
wrote-out = Écrit { $name }
export-the-profile = exporter le profil
kept-the-curve-as = A conservé la courbe comme { $name }
keep-as-a-profile = garder la courbe comme un profil
discarded = Éliminé { $name }
discarded-the-own-curve = Éliminé la courbe de l’appareil
discard-that-curve = Jeter cette courbe
read-the-catalogue = lire le catalogue AutoEq
kept-from-autoeq = conservé { $name }, { $bands ->
    [one] { $bands } bande
   *[other] { $bands } bandes
}
no-autoeq-measurement = AutoEq n’a pas de mesure pour ce dispositif
frequency-refused = une bande se situe entre 1 Hz et 40 kHz
gain-refused = Le gain d’un groupe se situe entre -40 et +40 dB
q-refused = a Q se situe entre 0,10 et 40,00
