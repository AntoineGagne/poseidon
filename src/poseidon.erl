-module(poseidon).

%% API
-export([
    new/1,
    stop/1,
    send/2,
    prepare_query/2
]).

%%%===================================================================
%%% API
%%%===================================================================

new(Config) ->
    {ok, Reference} = poseidon_nif:client_start(Config),
    receive
        ok ->
            {ok, Reference};
        {error, _} = Error ->
            Error
    end.

stop(Reference) ->
    poseidon_nif:client_stop(Reference).

send(Reference, Message) ->
    ok = poseidon_nif:client_send(Reference, Message),
    receive
        Response -> Response
    end.

prepare_query(Reference, Query) ->
    ok = poseidon_nif:client_prepare_query(Reference, Query),
    receive
        Response -> Response
    end.

%%%===================================================================
%%% Internal functions
%%%===================================================================
