what-the-rankings-count = Les classements comptent les chansons encore dans votre bibliothèque.
nothing-played = Rien n’a été joué dans { NUMBER($year, useGrouping: "false") }
play-something = Joue quelque chose et ça se remplit.
song-of-the-year = Votre chanson de l’année
artist-of-the-year = Votre artiste de l’année
album-of-the-year = Votre album de l’année
genre-of-the-year = Votre genre de l’année
my-year = Mon { NUMBER($year, useGrouping: "false") } sur Resonate
of-music = { $listened } de musique
top-song = Meilleure chanson : { $song }
top-artist = Haut artiste : { $artist }
top-album = Meilleur album : { $album }
top-genre = Meilleur genre : { $genre }
longest-streak = { $count ->
    [one] Tranche la plus longue: { $count } jour
   *[other] Tranche la plus longue: { $count } jours
}
lossless = { $share } sans perte
across-the-days = { $across }, sur { $days }
your-year = Votre année
biggest-month = { $month } était votre plus grand mois
your-day = Ton jour
listened-most-at = Vous avez écouté tout le monde à { $hour }:00
biggest-day-was = Votre plus grand jour était { $date }: { $listened }
your-streak = Votre stries
in-a-row = { $days } dans une rangée
streak-from = De { $date }. { $biggest }
your-biggest-day = Ton plus grand jour
joined-your-library = { $songs } a rejoint votre bibliothèque.
new-to-you = Nouveau pour toi
heard-for-the-first-time = { $songs } entendu pour la première fois
new-to-your-library = Nouveauté dans votre bibliothèque
your-sound = Votre son
above-cd-quality = { $share } au-dessus de la qualité du CD. Compte tenu des chansons de votre bibliothèque.
where-it-began = Où ça a commencé
by-on = { $by }, le { $on }
only-on = Sur { $on }
that-was-your-year = C’était votre { NUMBER($year, useGrouping: "false") }
named-by = { $name } par { $by }
songs = { $count ->
    [one] { $count } chanson
   *[other] { $count } chansons
}
artists = { $count ->
    [one] { $count } artiste
   *[other] { $count } artistes
}
albums = { $count ->
    [one] { $count } album
   *[other] { $count } albums
}
days = { $count ->
    [one] { $count } jour
   *[other] { $count } jours
}
minutes = { $count ->
    [one] { $count } minute
   *[other] { $count } minutes
}
hours = { $count ->
    [one] { $count } heure
   *[other] { $count } heures
}
small-hours = Les petites heures sont à vous.
start-the-day = Tu commences la journée avec de la musique.
mornings = Vos matins ont une bande son.
afternoons = Vos après-midi ont une bande son.
evenings = Vos soirées ont une bande son.
late = Tu écoutes tard.
