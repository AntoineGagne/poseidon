-module(poseidon).

%% API
-export([start/0]).

%%%===================================================================
%%% API
%%%===================================================================

-spec start() -> ok.
start() ->
    poseidon_nif:start().

%%%===================================================================
%%% Internal functions
%%%===================================================================
