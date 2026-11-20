every-other-device = tout autre dispositif
its-own-curve = sa propre courbe
unreadable = illisible
measured = mesurée
switched = égaliseur: { $state }
on = le
off = Départ
own-curve-of = la courbe propre de { $owner }
bound = lié { $binding } à { $sink }
unbound = non consolidé { $sink }
shaped-from = en forme de { $own } à partir de { $from }, { $bands ->
    [one] { $bands } bande
   *[other] { $bands } bandes
}
kept-graphic = a maintenu une courbe graphique comme { $name }, { $bands ->
    [one] { $bands } bande
   *[other] { $bands } bandes
} à { $rate } et rééquipé à nouveau à la vitesse qu’un flux joue à
kept = conservé { $name }, { $bands ->
    [one] { $bands } bande
   *[other] { $bands } bandes
}
the-layout-file = le { $layout } { $file }
fitted-toward = { $bands ->
    [one] { $bands } bande
   *[other] { $bands } bandes
} montés vers { $target }, { $before } dB avant et { $after } dB après
shaped-from-told = en forme { $own } à partir de { $origin }: { $told }
kept-from-told = conservé { $name } à partir de { $origin }: { $told }
what-the-desktop-plays = ce que le bureau joue
the-default-microphone = le microphone par défaut
the-default-output = la sortie par défaut
playing-a-sweep = en jouant un balayage { $through } et en écoutant { $heard } pour { $seconds } s
wrote-the-measurement = écrit la mesure à { $path }
the-sweep = le balayage qu’il a mesuré
nothing-measured = rien n’a mesuré les réponses à cela
no-correction = { $device }: aucune correction n’est retenue
not-fetched = { $device }: elle n’a pas pu être récupérée
kept-corrections = conservé { $kept } des { $asked ->
    [one] { $asked } correction
   *[other] { $asked } corrections
}
measurement-passed-over = { $lines ->
    [one] { $lines } ligne de la mesure a été dépassée
   *[other] { $lines } les lignes de mesure ont été dépassées
}
replaced = { $name } a déjà été conservé et est remplacé
lines-passed-over = { $lines ->
    [one] { $lines } la ligne a été dépassée
   *[other] { $lines } lignes ont été dépassées
}
filters-approximated = { $filters ->
    [one] { $filters } Le filtre nomme un rolloff cette construction se lit comme deuxième ordre
   *[other] { $filters } filtres nom un rolloff cette construction se lit comme deuxième ordre
}
values-clamped = { $values ->
    [one] { $values } valeur au-delà de ce que cette construction détient a été tenue à ses limites
   *[other] { $values } valeurs passées ce que cette construction détient ont été maintenues à ses limites
}
wrote = écrit { $spoken } à { $path }
forgot = Oublié { $name }
no-profiles = aucun profil n’est conservé dans { $folder }
own-curve-heading = ENSEMBLE DES
bands-heading = BANDES
preamp-heading = PRÉAMBULE
bound-heading = FOND
yes = oui
profile-heading = PROFIL
bound-to-heading = POUR
device-heading = DÉVISEZ
nothing-bound = rien n’est lié à aucun appareil
bound-but-not-kept = { $spoken } est lié mais aucun profil de ce nom n’est conservé
for-sink = pour { $sink }: { $spoken }
preamp = Préampli: { $preamp }
fitted-to-graphic = monté sur une courbe graphique de { $points } points: les bandes ci-dessous sont sa position à { $rate }, et un courant à une autre vitesse est monté à nouveau à sa propre
holds-no-bands = il ne tient pas de bandes
type-heading = TYPE
frequency-heading = FRÉQUENCE
gain-heading = FEUILLES
peak = pic: { $peak } dB
measured-by-heading = MESUREE PAR
rig-heading = RIG
path-heading = PATH
measurer-missed = { $asked } n’a rien mesuré pour elle : { $chosen }’s est le meilleur classement
sink = sortie audio: { $description } ({ $name })
looks-like = ressemble à : { $device }
fetch-it = chercher: resonate eq --fetch « { $device } » --for { $sink }
not-clear-enough = rien ne mesurait les réponses à ce nom assez clairement pour qu’il soit utile de deviner à
