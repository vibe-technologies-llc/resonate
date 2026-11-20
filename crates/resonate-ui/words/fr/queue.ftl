remove-hint = Sortez cette rangée de la file d’attente.
remove-reached-hint = Prendre chaque ligne atteinte hors de la file d’attente
queued-hint = Mettre chaque rangée en file d’attente dans une playlist
clear-hint = Sortez chaque rangée de la file d’attente et arrêtez-vous
history = Historique
now-playing = Lecture en cours
playing-next = Lecture suivante
continue-playing = Continuez à jouer
empty = La file d’attente est vide.
how-to-fill = Jouez quelque chose à partir d’albums ou de pistes, ou ajoutez des lignes avec les marques de file d’attente.
tracks = { $count ->
    [one] { $count } piste
   *[other] { $count } voies
}
at-least = au moins { $length }
on-row = à la ligne { $row }
waiting = { $count } pour jouer ensuite
continue-from = Continuer depuis { $device }
continue-from-hint = Jouer les { $songs } chansons { $device } jouaient, de celui sur lequel il était
queue = File d’attente
clear = Effacer
save-as-a-playlist = Enregistrer comme liste de lecture
put-back = Retirez-vous
take-out-again = Retirez-vous.
took-out = { $count ->
    [one] A pris { $count } trace hors de la file d’attente
   *[other] Took { $count } pistes hors de la file d’attente
}
puts-back = Placez les lignes prises en dernier lieu hors de la file d’attente où elles étaient. { $behind ->
    [0] C’est le dernier qu’il faut remettre
    [one] Un geste de plus est derrière lui
   *[other] D’autres gestes sont derrière elle
}
takes-out-again = Reprenez les lignes mises en attente. { $behind ->
    [0] C’est le dernier qu’il y ait à reprendre.
    [one] Une dernière mise en arrière est derrière elle
   *[other] D’autres mises en arrière sont derrière elle
}
suggested-from = Suggestion · { $service }
download-song-hint = Téléchargez cette chanson suggérée dans votre bibliothèque
downloaded-song-hint = En attente de téléchargement
download-to-library = Télécharger dans la bibliothèque
mix-in-suggestions = Mélanger les suggestions
mix-in-suggestions-hint = Weave a suggéré des chansons en streaming dans la file d’attente à venir
mixed-in-suggestions = { $count ->
    [one] Mixte { $count } a suggéré une chanson dans la file d’attente
   *[other] Mixte { $count } a suggéré des chansons dans la file d’attente
}
no-queue-suggestions = Aucune nouvelle chanson suggérée n’a été trouvée pour cette file d’attente
