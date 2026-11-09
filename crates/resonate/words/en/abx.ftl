help-hearing = a  hear A      b  hear B      x  hear X
help-answering = 1  X is A      2  X is B      q  quit        ?  this help
trial = trial { $number } of { $planned }
levels-matched = levels matched to { $residual } dB; { $trial }
one-level-played = A and B play at one level; { $trial }
one-level-measured = A and B measured at one level; { $trial }
louder-lowered = the louder arm is lowered by { $by } dB to match the other; { $trial }
right = right
wrong = wrong
none-answered = no trial was answered
stopped-after = stopped after { $answered } of { $planned ->
    [one] { $planned } trial
   *[other] { $planned } trials
}
heard-it = that is unlikely to be luck: you can hear this
luck = that could well be luck: no difference was shown
scored = { $right } of { $answered } right, chance of that by guessing { $percent }% (p = { $p })
measuring = measuring how loud A and B play the song, to match them
failed = { $track } failed
rejected = { $command } rejected
hearing = hearing { $arm }
