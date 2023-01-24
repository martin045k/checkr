/*
 Generated by typeshare 1.0.0
*/

export interface AnalysisRequest {
	analysis: Analysis;
	src: string;
	input: string;
}

export interface AnalysisResponse {
	stdout: string;
	stderr: string;
	parsed_markdown?: string;
	took: string;
	validation_result?: ValidationResult;
}

export interface GraphRequest {
	src: string;
	deterministic: boolean;
}

export interface GraphResponse {
	dot?: string;
}

export interface CompilationStatus {
	compiled_at: number;
	state: CompilerState;
}

export interface Sample {
	input_json: unknown;
	input_markdown: string;
	output_markdown: string;
}

export interface Env {
	analysis: Analysis;
	sample: Sample;
}

export interface Generation {
	program: string;
	dot: string;
	envs: Env[];
}

export type ValidationResult = 
	| { type: "CorrectTerminated", content?: undefined }
	| { type: "CorrectNonTerminated", content: {
	iterations: number;
}}
	| { type: "Mismatch", content: {
	reason: string;
}}
	| { type: "TimeOut", content?: undefined };

export enum CompilerState {
	Compiling = "Compiling",
	Compiled = "Compiled",
	CompileError = "CompileError",
}

export enum Analysis {
	Graph = "graph",
	Interpreter = "interpreter",
	ProgramVerification = "program-verification",
	Sign = "sign",
	Security = "security",
}

