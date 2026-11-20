taken-up = { $count ->
    [one] { $players } est en cours d’exécution et reprend ceci
   *[other] { $players } sont en cours d’exécution et prennent ce
}
kept = { $count ->
    [one] { $players } est en cours d’exécution et conserve les paramètres avec lesquels il a commencé; il n’entend cela que s’il lit ce fichier de paramètres, une fois qu’il a été redémarré
   *[other] { $players } sont en cours d’exécution et gardent les paramètres avec lesquels ils ont commencé; ils n’entendent cela que s’ils lisent ce fichier de paramètres, une fois qu’ils sont recommencés
}
