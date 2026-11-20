the-decade = { $decade ->
    [1920] des années 20
    [1930] des années 30
    [1940] des années 40
    [1950] des années 50
    [1960] les années soixante
    [1970] des années 70
    [1980] les années 80
    [1990] les années 90
   *[other] les { NUMBER($decade, useGrouping: "false") }s
}
decade-named = { $decade ->
    [1920] Les années 20
    [1930] Les années 30
    [1940] Les années 40
    [1950] Les années 50
    [1960] Les années 60
    [1970] Les années 70
    [1980] Les années 80
    [1990] Les années 90
   *[other] Les { NUMBER($decade, useGrouping: "false") }s
}
genre-of-a-decade = { $genre } de { $decade }
more-like = Plus comme { $seed }
deep-cuts = { $artist }, découpes profondes
discover = Découvrir { $artist }
back-to = Retour à { $artist }
unheard-genre = Non entendu { $genre }
on-repeat = En répétant
forgotten-favourites = Favoris oubliés
old-flames = Vieilles flammes
second-chances = Deuxième chance
starred-never-played = Starred, jamais joué
fresh-arrivals = Arrivées fraîches
short-and-sharp = Court et aiguisé
surround = Surround
in-mono = En mono
slow-burners = Brûleurs lents
never-heard = Jamais entendu
most-played = La plupart joués
recently-added = Dernièrement ajouté
back-catalogue = Catalogue précédent
hi-res = Bonjour.
long-players = Joueurs longs
reason-decade = Tout de { $decade }
reason-genre-of-a-decade = Un genre, de { $decade }
reason-genre = Tout ce qui est déposé sous elle
reason-artist = Tout ce qu’ils ont fait
reason-never-heard = Tu n’as jamais entendu ça.
reason-most-played = Ceux à qui tu reviens
reason-recently-added = Les plus récents de la bibliothèque
reason-fresh-arrivals = Nouveau ce mois-ci et pas encore joué
reason-back-catalogue = Pas entendu depuis plus d’un an
reason-on-repeat = Ce que tu continues à retourner à ce mois-ci
reason-kindred-artists = Des artistes partageant ses genres, que vous entendez moins
reason-deep-cuts = Ce que vous passez par un artiste que vous jouez
reason-artist-to-discover = Un artiste que tu tiens et que tu n’as jamais joué
reason-unheard-genre = Un genre que vous jouez, dans des chansons que vous n’avez pas
reason-forgotten-favourites = Étoilé, puis laissé seul pendant un demi-année
reason-old-flames = Joué souvent une fois, calme pendant une demi-année
reason-second-chances = Joué une fois, il y a plus d’un mois
reason-starred-unheard = Vous les avez marqués avant de les entendre.
reason-artist-left-behind = Tu les jouais souvent, puis tu t’arrêtais
reason-hi-res = Mieux qu’un CD
reason-long-players = Dix minutes et plus
reason-slow-burners = Six à dix minutes
reason-short-and-sharp = Deux minutes ou moins
reason-surround = Plus de deux chaînes
reason-mono = Une chaîne
kind-for-you = Fait pour toi
kind-rediscover = Redécouverte
kind-listening = De ton écoute
kind-eras = Éras
kind-genres = Genres
kind-artists = Artistes
kind-sound = Son
