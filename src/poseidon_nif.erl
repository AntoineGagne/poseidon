-module(poseidon_nif).

-include("crates.hrl").

%% API
-export([
    client_start/1,
    client_stop/1,
    client_send/2,
    client_prepare_query/2
]).

%% Native library support
-export([load/0]).

-on_load(load/0).

%%%===================================================================
%%% API
%%%===================================================================

client_start(_Config) ->
    not_loaded(?LINE).

client_stop(_Reference) ->
    not_loaded(?LINE).

client_send(_Reference, _Message) ->
    not_loaded(?LINE).

client_prepare_query(_Reference, _Query) ->
    not_loaded(?LINE).

%%%===================================================================
%%% NIF functions
%%%===================================================================

-spec load() -> ok | {error, term()}.
load() ->
    ?load_nif_from_crate(poseidon, ?crate_poseidon_nif, 0).

%%%===================================================================
%%% Internal functions
%%%===================================================================

not_loaded(Line) ->
    erlang:nif_error({error, {not_loaded, [{module, ?MODULE}, {line, Line}]}}).
