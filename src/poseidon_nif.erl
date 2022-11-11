-module(poseidon_nif).

-include("crates.hrl").

%% API
-export([start/0]).

%% Native library support
-export([load/0]).

-on_load(load/0).

%%%===================================================================
%%% API
%%%===================================================================

-spec start() -> ok.
start() ->
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
