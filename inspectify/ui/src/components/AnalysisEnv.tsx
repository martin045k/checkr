import React, { useRef } from "react";
import { useEffect, useState } from "react";
import * as wasm from "../../../wasm/pkg/wasm";
import deepEqual from "deep-equal";
import { ArrowPathRoundedSquareIcon } from "@heroicons/react/24/outline";
import * as api from "../lib/api";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Analysis,
  AnalysisResponse,
  CompilationStatus,
  CompilerState,
} from "../lib/types";
import { StretchEditor } from "./StretchEditor";
import { Indicator, IndicatorState, INDICATOR_TEXT_COLOR } from "./Indicator";
import { capitalCase } from "change-case";

const searchParams = new URL(document.location.toString()).searchParams;

const inputted: { analysis?: string; src?: string; input?: string } =
  Object.fromEntries(searchParams.entries());

const ENVS = Object.values(Analysis).filter(
  (a) => a != Analysis.Graph
) satisfies Analysis[];
const ANALYSIS_NAMES = Object.fromEntries(
  ENVS.map((env) => [env, capitalCase(env)] satisfies [Analysis, string])
) as Record<Analysis, string>;

type GraphShown = "graph" | "reference";

export const AnalysisEnv = () => {
  const [deterministic, setDeterministic] = useState(true);
  const [env, setEnv] = useState<Analysis>(
    inputted.analysis && (ENVS as string[]).includes(inputted.analysis)
      ? (inputted.analysis as Analysis)
      : Analysis.Interpreter
  );
  const [src, setSrc] = useState(inputted.src ?? wasm.generate_program(env));
  const [graphShown, setGraphShown] = useState<GraphShown>("graph");
  const [dotReference, setDotReference] = useState<null | string>(null);
  const [dotGraph, setDotGraph] = useState<null | string>(null);

  useEffect(() => {
    setDotReference(wasm.dot(deterministic, src));

    const { abort, promise } = api.graph({ deterministic, src });

    promise
      .then((res) => {
        setDotGraph(res.dot ?? null);
      })
      .catch((e) => {
        if (e.name != "AbortError") console.error("analysis error:", e);
      });

    return () => abort();
  }, [deterministic, src]);

  return (
    <div className="grid min-h-0 grid-cols-[1fr_2fr] grid-rows-[1fr_auto_auto_1fr]">
      <div className="grid grid-rows-[auto_1fr] divide-y divide-slate-600">
        <div className="grid grid-cols-3 divide-x divide-slate-600 border-r border-slate-600">
          <button
            className="flex items-center justify-center space-x-1 bg-slate-800 py-1 px-1.5 text-sm text-white transition hover:bg-slate-700 active:bg-slate-900"
            onClick={() => {
              setSrc(wasm.generate_program(env));
            }}
          >
            <span>Generate</span>
            <ArrowPathRoundedSquareIcon className="w-4" />
          </button>
          <label
            htmlFor="determinism"
            className="flex select-none items-center justify-center space-x-2 bg-slate-800 py-1 px-1.5 text-sm text-white transition hover:bg-slate-700 active:bg-slate-900"
          >
            <span>Determinism</span>
            <input
              type="checkbox"
              name="determinism"
              id="determinism"
              checked={deterministic}
              onChange={(e) => setDeterministic(e.target.checked)}
            />
          </label>
          <select
            className="flex appearance-none items-center justify-center space-x-1 rounded-none bg-slate-800 py-1 px-1.5 text-center text-sm text-white transition hover:bg-slate-700 active:bg-slate-900"
            value={env}
            onChange={(e) => setEnv(e.target.value as Analysis)}
          >
            {ENVS.map((e) => (
              <option key={e} value={e}>
                {ANALYSIS_NAMES[e]}
              </option>
            ))}
          </select>
        </div>
        <div className="relative">
          <StretchEditor source={src} onChange={setSrc} />
        </div>
      </div>
      <div className="relative row-span-2 bg-slate-800 text-xs">
        <div className="absolute top-4 right-6 flex flex-col space-y-2">
          <button
            onClick={() => setGraphShown("graph")}
            className={
              "z-10 flex aspect-square w-7 items-center justify-center rounded-full border border-current p-1 text-center transition hover:text-slate-200 " +
              (graphShown == "graph" ? "text-white" : "text-slate-600")
            }
          >
            G
          </button>
          <button
            onClick={() => setGraphShown("reference")}
            className={
              "z-10 flex aspect-square w-7 items-center justify-center rounded-full border border-current p-1 text-center transition hover:text-slate-200 " +
              (graphShown == "reference" ? "text-white" : "text-slate-600")
            }
          >
            R
          </button>
        </div>
        {graphShown == "graph"
          ? dotGraph && <Network dot={dotGraph} />
          : dotReference && <Network dot={dotReference} />}
      </div>
      <Env env={env} src={src} />
    </div>
  );
};

type RightTab = "reference" | "stdout" | "stderr" | "validation";
const RIGHT_TABS_LABEL = {
  reference: "Reference output",
  stdout: "Raw output",
  stderr: "Debug output",
  validation: "Validation result",
} satisfies Record<RightTab, string>;

const Env = ({ env, src }: { env: Analysis; src: string }) => {
  const [input, setInput] = useState<wasm.Input | null>(null);
  const [output, setOutput] = useState<wasm.Output | null>(null);

  const realReferenceOutput = output?.markdown ?? "";

  const [referenceOutput, setReferenceOutput] = useState(realReferenceOutput);
  const [tab, setTab] = useState<RightTab>("reference");
  const [inFlight, setInFlight] = useState(false);
  const [response, setResponse] = useState<null | AnalysisResponse>(null);
  const [compilationStatus, setCompilationStatus] =
    useState<null | CompilationStatus>(null);

  const realReferenceOutputRef = useRef(realReferenceOutput);
  realReferenceOutputRef.current = realReferenceOutput;

  useEffect(() => {
    if (input || !inputted.input) return;

    try {
      const fullInput = wasm.complete_input_from_json(env, inputted.input);
      setInput(fullInput);
    } catch (e) {
      console.error(e);
    }
  }, [env, input]);

  useEffect(() => {
    const aborts = [] as (() => void)[];

    const interval = setInterval(() => {
      aborts.forEach((a) => a());
      aborts.slice(0, aborts.length);
      const { abort, promise } = api.compilationStatus();
      aborts.push(abort);
      promise
        .then((res) => {
          setCompilationStatus((old) => {
            if (deepEqual(old, res)) return old;
            console.log("got new");
            return res;
          });
        })
        .catch((e) => {
          if (e.name != "AbortError") console.error("analysis error:", e);
        });
    }, 200);

    return () => {
      aborts.forEach((a) => a());
      aborts.splice(0, aborts.length);
      clearInterval(interval);
    };
  }, []);

  useEffect(() => {
    if (
      !input ||
      !compilationStatus ||
      compilationStatus.state != CompilerState.Compiled
    )
      return;

    if (input.analysis != env) {
      console.info(
        `not analyzing, since input was generated for ${input.analysis}, while current is ${env}`
      );
      return;
    }

    setInFlight(true);

    const { promise, abort } = api.analyze({
      analysis: env,
      input: input.json,
      src,
    });

    promise
      .then((res) => {
        setInFlight(false);
        setResponse(res);
        setReferenceOutput(realReferenceOutputRef.current);
      })
      .catch((e) => {
        if (e.name != "AbortError") console.error("analysis error:", e);
      });

    return () => abort();
  }, [compilationStatus, src, input]);

  useEffect(() => {
    if (
      (inputted.input ? input : false) &&
      (input ? input.analysis != env : false)
    ) {
      try {
        const input = wasm.generate_input_for(src, env);
        setInput(input);
      } catch (e) {
        console.error(e);
      }
    }
  }, [env, input, src]);

  useEffect(() => {
    if (!input) return;

    try {
      const output = wasm.run_analysis(src, input);
      setOutput(output);
    } catch (e) {
      console.error(e);
    }
  }, [src, env, input]);

  const indicatorState =
    inFlight || compilationStatus?.state != CompilerState.Compiled
      ? IndicatorState.Working
      : response
      ? response.validation_result
        ? response.validation_result.type == "CorrectTerminated"
          ? IndicatorState.Correct
          : response.validation_result.type == "CorrectNonTerminated"
          ? IndicatorState.Correct
          : response.validation_result.type == "Mismatch"
          ? IndicatorState.Mismatch
          : response.validation_result.type == "TimeOut"
          ? IndicatorState.TimeOut
          : IndicatorState.Working
        : IndicatorState.Error
      : IndicatorState.Error;

  return (
    <>
      <div className="grid place-items-start border-t border-slate-500 bg-slate-800 px-4 py-3 text-xl">
        <div className="prose prose-invert">
          <ReactMarkdown
            children={input?.markdown ?? ""}
            remarkPlugins={[remarkGfm]}
          />
        </div>
      </div>
      <div
        className={
          "relative col-span-full w-full border-t-4 border-current transition " +
          INDICATOR_TEXT_COLOR[indicatorState]
        }
      >
        <div className="absolute right-0 top-0 z-10 -translate-y-full">
          <Indicator state={indicatorState} />
        </div>
      </div>
      {response ? (
        <div
          className={
            "relative col-span-2 grid grid-cols-2 transition duration-700 " +
            (inFlight ? "blur-sm delay-100" : "")
          }
        >
          {/* <div className="absolute inset-0 grid grid-cols-[1fr_2fr] divide-slate-600 overflow-y-auto"> */}
          <div className="absolute inset-0 grid grid-cols-2 justify-center divide-slate-600 overflow-y-auto bg-slate-800">
            {response.validation_result ? (
              <div className="flex w-full max-w-prose flex-col space-y-2 bg-slate-800 px-4 py-2 text-xl text-white">
                <h3 className="text-lg">Output</h3>
                <div className="prose prose-invert w-full max-w-none prose-table:w-full">
                  <ReactMarkdown
                    children={response.parsed_markdown ?? ""}
                    remarkPlugins={[remarkGfm]}
                  />
                </div>
              </div>
            ) : (
              <div className="flux w-full space-y-2 px-4 py-2">
                <h3 className="text-lg font-bold italic text-white">Error</h3>
                <div
                  className="prose prose-invert w-full max-w-none prose-pre:whitespace-pre-wrap prose-table:w-full"
                  title={JSON.stringify(response.stderr.trim())}
                >
                  <ReactMarkdown
                    children={"````bash\n" + response.stderr.trim()}
                    remarkPlugins={[remarkGfm]}
                  />
                </div>
              </div>
            )}
            <div className="flex w-full max-w-prose flex-col space-y-2 bg-slate-800 px-4 py-2 text-xl text-white">
              <select
                className="flex appearance-none bg-transparent text-lg"
                value={tab}
                onChange={(e) => setTab(e.target.value as RightTab)}
              >
                {Object.entries(RIGHT_TABS_LABEL).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
              <div className="prose prose-invert w-full max-w-none prose-table:w-full">
                {tab == "reference" ? (
                  <ReactMarkdown
                    children={referenceOutput}
                    remarkPlugins={[remarkGfm]}
                  />
                ) : tab == "stderr" ? (
                  <pre className="whitespace-pre-wrap">{response.stderr}</pre>
                ) : tab == "stdout" ? (
                  <pre className="whitespace-pre-wrap">{response.stdout}</pre>
                ) : tab == "validation" ? (
                  <pre className="whitespace-pre-wrap">
                    {response.validation_result
                      ? JSON.stringify(response.validation_result, null, 2)
                      : ""}
                  </pre>
                ) : null}
              </div>
            </div>
          </div>
        </div>
      ) : (
        <div className="col-span-2 grid place-items-center text-4xl">
          <div className="animate-bounce">👻</div>
        </div>
      )}
    </>
  );
};

export const Network = React.memo(({ dot }: { dot: string }) => {
  const [container, setContainer] = React.useState<null | HTMLDivElement>();

  React.useEffect(() => {
    if (!container) return;

    const run = async () => {
      const visPromise = import("vis-network/esnext");
      const vis = await visPromise;

      const data = vis.parseDOTNetwork(dot);

      new vis.Network(container, data, {
        interaction: { zoomView: false },
        nodes: {
          color: {
            background: "#666666",
            border: "#8080a0",
            highlight: "#80a0ff",
          },
          font: {
            color: "white",
          },
          borderWidth: 1,
          shape: "circle",
          size: 30,
        },
        edges: {
          color: "#D0D0FF",
          font: {
            color: "white",
            strokeColor: "#200020",
          },
        },
        autoResize: true,
      });
    };

    const debounce = requestAnimationFrame(() => run().catch(console.error));
    return () => cancelAnimationFrame(debounce);
  }, [container, dot]);

  return <div className="h-full w-full" ref={setContainer}></div>;
});
