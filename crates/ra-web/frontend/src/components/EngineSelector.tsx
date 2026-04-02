import {
  FormControl,
  InputLabel,
  Select,
  MenuItem,
  type SelectChangeEvent,
} from '@mui/material';
import { ENGINES } from '../constants';
import type { Engine } from '../types';

interface EngineSelectorProps {
  value: Engine;
  onChange: (engine: Engine) => void;
  label?: string;
}

export function EngineSelector({
  value,
  onChange,
  label = 'Engine',
}: EngineSelectorProps) {
  const handleChange = (event: SelectChangeEvent) => {
    onChange(event.target.value as Engine);
  };

  return (
    <FormControl size="small" fullWidth>
      <InputLabel>{label}</InputLabel>
      <Select value={value} label={label} onChange={handleChange}>
        {ENGINES.map(engine => (
          <MenuItem key={engine.id} value={engine.id}>
            {engine.name} {engine.version}
          </MenuItem>
        ))}
      </Select>
    </FormControl>
  );
}
