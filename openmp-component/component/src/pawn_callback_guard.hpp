#pragma once

// The SDK releases argument strings, but a partial PushOne failure can leave
// scalar arguments and paramcount on the AMX stack. Restore all call-local
// allocation state on every exit, including failed pushes and Pawn errors.
template <typename Script>
class PawnCallbackGuard final
{
public:
	explicit PawnCallbackGuard(Script& script)
		: script_(script), heap_(script.GetHEA()), stack_(script.GetSTK()),
		  parameterCount_(script.GetAMX()->paramcount)
	{
	}

	~PawnCallbackGuard()
	{
		script_.Release(heap_);
		script_.SetSTK(stack_);
		script_.GetAMX()->paramcount = parameterCount_;
	}

	PawnCallbackGuard(const PawnCallbackGuard&) = delete;
	PawnCallbackGuard& operator=(const PawnCallbackGuard&) = delete;

private:
	Script& script_;
	decltype(script_.GetHEA()) heap_;
	decltype(script_.GetSTK()) stack_;
	int parameterCount_;
};
